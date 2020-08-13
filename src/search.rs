use crate::buffer::async_line_buffer::{AsyncLineBufferBuilder, AsyncLineBufferReader};
use crate::buffer::BufferPool;
use crate::error::{Error, Result};
use crate::matcher::Matcher;
use crate::print::{PrintMessage, PrintableResult, PrinterSender};
use crate::target::Target;
use async_std::fs::{self, File};
use async_std::io::{BufReader, Read};
use async_std::path::Path;
use async_std::prelude::*;
use async_std::sync::Arc;
use std::collections::VecDeque;
use std::time::Instant;

// How many bytes must we check to be reasonably sure the input isn't binary?
const BINARY_CHECK_LEN_BYTES: usize = 512;

pub(crate) mod stats {
    use std::time::Duration;

    #[derive(Debug, Default)]
    pub(crate) struct ReadStats {
        /// The count of total files encountered during search.
        /// Includes skipped non-utf8 files.
        pub(crate) total_files_visited: usize,

        /// Count of files skipped as non-utf8.
        /// For stats coming from "single file level" reads, this is 1
        /// if the file was skipped or 0 if it was not.
        /// Coming from "aggregate" reads, this will be the count of all
        /// files skiped at that level of aggregation.
        pub(crate) skipped_files_non_utf8: usize,

        /// How many bytes were checked to determine the file is or is not utf8.
        pub(crate) non_utf8_bytes_checked: usize,

        /// Count of lines that matched during reading.
        pub(crate) lines_matched_count: usize,

        /// Count of summed byte-length of lines that matched during reading.
        pub(crate) lines_matched_bytes: usize,

        /// The duration of time spent recursing through the filesystem.
        pub(crate) filesystem_walk_dur: Duration,

        /// The duration of time spent searching a reader.
        /// Might be an aggregated time (if the returning method searches multiple readers)
        /// or a time for only one reader (if the returning method only searches one reader).
        pub(crate) reader_search_dur: Duration,

        pub(crate) max_buffer_size: usize,

        pub(crate) buffers_created: usize,
    }

    impl ReadStats {
        pub(super) fn fold_in(&mut self, other: &ReadStats) {
            self.total_files_visited += other.total_files_visited;
            self.skipped_files_non_utf8 += other.skipped_files_non_utf8;
            self.non_utf8_bytes_checked += other.non_utf8_bytes_checked;
            self.lines_matched_count += other.lines_matched_count;
            self.lines_matched_bytes += other.lines_matched_bytes;
            self.filesystem_walk_dur += other.filesystem_walk_dur;
            self.reader_search_dur += other.reader_search_dur;
            self.max_buffer_size = usize::max(self.max_buffer_size, other.max_buffer_size);
            self.buffers_created += other.buffers_created;
        }
    }
}

pub(crate) struct SearcherBuilder<M, P>
where
    M: Matcher,
    P: PrinterSender,
{
    matcher: M,
    printer: P,
}

impl<M, P> SearcherBuilder<M, P>
where
    M: Matcher + Sync,
    P: PrinterSender + Sync,
{
    pub(crate) fn new(matcher: M, printer: P) -> SearcherBuilder<M, P> {
        Self { matcher, printer }
    }

    pub(crate) fn build(self) -> Searcher<M, P> {
        Searcher::new(self.matcher, self.printer)
    }
}

pub(crate) struct Searcher<M, P>
where
    M: Matcher + 'static,
    P: PrinterSender + Sync + 'static,
{
    matcher: M,
    printer: P,
}

impl<M, P> Searcher<M, P>
where
    M: Matcher + Sync + 'static,
    P: PrinterSender + Sync + 'static,
{
    fn new(matcher: M, printer: P) -> Self {
        Self { matcher, printer }
    }

    pub(crate) async fn search(&'_ self, targets: &'_ [Target]) -> Result<stats::ReadStats> {
        use async_crawl::{AsyncCrawler, Crawler};
        // let crawler = async_crawl::singlethread_crawler::make_crawler();
        // let crawler = async_crawl::async_scaled_crawler::make_crawler(6);
        let crawler = async_crawl::async_recursive_crawler::make_crawler();

        let target = &targets[0];

        let path = match target {
            Target::Path(pathbuf) => pathbuf,
            Target::Stdin => panic!("Stdin not supported right now."),
        };

        let path: std::path::PathBuf = path.into();

        let buf_pool = Arc::new(BufferPool::new());
        let printer = self.printer.clone();
        let matcher = self.matcher.clone();

        crawler
            .crawl(&path, move |p| async move {
                // dbg!("Crawling path.");
                Searcher::search_file(&p.path(), matcher, printer, buf_pool).await;
            })
            .await;

        Ok(stats::ReadStats::default())
    }

    /// Given some `Target`s, search them using the given `Matcher`
    /// and send the results to the given `Printer`.
    /// `Ok` if every target is an available file or directory (or stdin).
    /// `Err` with a list of failed paths if any of the paths are invalid.
    pub(crate) async fn search_1(&self, targets: &'_ [Target]) -> Result<stats::ReadStats> {
        let mut agg_stats = stats::ReadStats::default();

        let mut error_paths = Vec::new();

        let buf_pool = Arc::new(BufferPool::new());

        for target in targets {
            let matcher = self.matcher.clone();
            let printer = self.printer.clone();

            let stats = match target {
                Target::Stdin => {
                    let file_rdr = BufReader::new(async_std::io::stdin());
                    let line_buf = AsyncLineBufferBuilder::new().build();

                    let mut line_rdr =
                        AsyncLineBufferReader::new(file_rdr, line_buf).line_nums(false);

                    Searcher::search_via_reader(matcher, &mut line_rdr, None, printer.clone()).await
                }
                Target::Path(path) => {
                    if path.is_file().await {
                        Searcher::search_file(path, matcher, printer, buf_pool.clone()).await
                    } else if path.is_dir().await {
                        Searcher::search_directory(path, matcher, printer, buf_pool.clone()).await
                    } else {
                        error_paths.push(format!("{}", path.display()));
                        stats::ReadStats::default()
                    }
                }
            };

            agg_stats.fold_in(&stats);
        }

        agg_stats.buffers_created = buf_pool.pool_size().await;

        if error_paths.is_empty() {
            Ok(agg_stats)
        } else {
            Err(Error::TargetsNotFound(error_paths))
        }
    }

    async fn search_via_reader<R>(
        matcher: M,
        buffer: &mut AsyncLineBufferReader<R>,
        name: Option<String>,
        printer: P,
    ) -> stats::ReadStats
    where
        R: Read + std::marker::Unpin,
    {
        use stats::ReadStats;

        let start = Instant::now();

        let mut binary_bytes_checked = 0;
        let mut stats = ReadStats::default();

        // This is the lowest level of granularity -- we are searching 1 file.
        stats.total_files_visited = 1;

        let name = name.unwrap_or_default();
        while let Some(line_result) = buffer.read_line().await {
            if binary_bytes_checked < BINARY_CHECK_LEN_BYTES {
                binary_bytes_checked += line_result.text().len();
                if !check_utf8(line_result.text()) {
                    stats.non_utf8_bytes_checked = binary_bytes_checked;
                    stats.skipped_files_non_utf8 = 1;
                    return stats;
                }
            }

            if matcher.is_match(line_result.text()) {
                stats.lines_matched_count += 1;
                stats.lines_matched_bytes += line_result.text().len();

                let printable = PrintableResult::new(
                    name.clone(),
                    line_result.line_num(),
                    line_result.text().into(),
                );
                printer.send(PrintMessage::Printable(printable));
            }
        }

        printer.send(PrintMessage::EndOfReading { target_name: name });

        drop(printer);

        stats.non_utf8_bytes_checked = binary_bytes_checked;
        stats.reader_search_dur = start.elapsed();
        stats.max_buffer_size = buffer.inner_buf_len();

        stats
    }

    async fn search_file(
        path: &Path,
        matcher: M,
        printer: P,
        buf_pool: Arc<BufferPool>,
    ) -> stats::ReadStats {
        let file = {
            let f = File::open(path).await;

            if let Ok(f) = f {
                f
            } else {
                return stats::ReadStats::default();
            }
        };

        let rdr = BufReader::new(file);

        let line_buf = buf_pool.acquire().await;

        let mut line_buf_rdr = AsyncLineBufferReader::new(rdr, line_buf).line_nums(true);

        let target_name = Some(path.to_string_lossy().to_string());

        let search_result =
            Searcher::search_via_reader(matcher, &mut line_buf_rdr, target_name, printer).await;

        buf_pool
            .return_to_pool(line_buf_rdr.take_line_buffer())
            .await;

        search_result
    }

    /// Given a directory path, descend down the whole tree,
    /// performing a search on every file found,
    /// and recursively visiting descendant directories.
    ///
    /// Note: this is a major simplification compared to how
    /// Ripgrep works. I believe it is possibly the biggest contributor
    /// to why Toygrep is much slower than Ripgrep on some workloads.
    ///
    /// Toygrep's current appraoch is this (as seen in the code below):
    /// An outermost loop, on one thread, descends through every directory,
    /// and fires off an async task whenever a file is encountered.
    ///
    /// In comparison, Ripgrip will spawn some number of workers, and they share
    /// a global queue of directories to visit, and descend independently of any outer loop.
    async fn search_directory(
        directory_path: &Path,
        matcher: M,
        printer: P,
        buf_pool: Arc<BufferPool>,
    ) -> stats::ReadStats {
        let start = Instant::now();

        let mut agg_stats = stats::ReadStats::default();

        let mut dir_stack = vec![];

        dir_stack.push(directory_path.to_path_buf());

        let mut spawned_tasks = vec![];

        while let Some(dir_path) = dir_stack.pop() {
            let mut dir_children = {
                if let Ok(children) = fs::read_dir(dir_path).await {
                    children
                } else {
                    continue;
                }
            };

            while let Some(Ok(dir_entry)) = dir_children.next().await {
                let meta = dir_entry.metadata().await.unwrap();

                if meta.is_file() {
                    let printer = printer.clone();
                    let matcher = matcher.clone();
                    let buf_pool = buf_pool.clone();

                    let task = async_std::task::spawn(async move {
                        let dir_child_path: &Path = &dir_entry.path();
                        Searcher::search_file(dir_child_path, matcher, printer, buf_pool).await
                    });

                    spawned_tasks.push(task);
                } else if meta.is_dir() {
                    dir_stack.push(dir_entry.path());
                }
            }
        }

        agg_stats.filesystem_walk_dur = start.elapsed();

        for task in spawned_tasks {
            let read_stats = task.await;
            agg_stats.fold_in(&read_stats);
        }

        agg_stats
    }
}

fn check_utf8(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok()
}
