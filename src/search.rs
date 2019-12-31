use crate::async_line_buffer::{AsyncLineBufferBuilder, AsyncLineBufferReader};
use crate::error::{Error, Result};
use crate::matcher::Matcher;
use crate::printer::threaded_printer::ThreadedPrinterSender;
use crate::printer::{PrintMessage, PrintableResult, PrinterSender};
use crate::target::Target;
use async_std::fs::{self, File};
use async_std::io::{BufReader, Read};
use async_std::path::{Path, PathBuf};
use async_std::prelude::*;
use walkdir::WalkDir;

// Buffers for files will be created with at least enough room to hold the
// whole file -- up until this maximum.
const MAX_BUFF_START_LEN: usize = 1_000_000;

// How many bytes must we check to be reasonably sure the input isn't binary?
const BINARY_CHECK_LEN_BYTES: usize = 512;

/// Given some `Target`s, search them using the given `Matcher`
/// and send the results to the given `Printer`.
/// `Ok` if every target is an available file or directory (or stdin).
/// `Err` with a list of failed paths if any of the paths are invalid.
pub(crate) async fn search_targets<M>(
    targets: &[Target],
    matcher: M,
    printer: ThreadedPrinterSender,
) -> Result<()>
where
    M: Matcher + 'static,
{
    let mut error_paths = Vec::new();

    for target in targets {
        let matcher = matcher.clone();
        let printer = printer.clone();

        match target {
            Target::Stdin => {
                let file_rdr = BufReader::new(async_std::io::stdin());
                let line_buf = AsyncLineBufferBuilder::new().build();

                let line_rdr = AsyncLineBufferReader::new(file_rdr, line_buf).line_nums(false);

                search_via_reader(matcher, line_rdr, None, printer.clone()).await?;
            }
            Target::Path(path) => {
                if path.is_file().await {
                    search_file(path, matcher, printer).await;
                } else if path.is_dir().await {
                    search_directory(path, matcher, printer).await;
                } else {
                    error_paths.push(format!("{}", path.display()));
                }
            }
        }
    }

    if error_paths.is_empty() {
        Ok(())
    } else {
        Err(Error::TargetsNotFound(error_paths))
    }
}

async fn search_directory<M>(directory_path: &Path, matcher: M, printer: ThreadedPrinterSender)
where
    M: Matcher + 'static,
{
    let mut spawned_tasks = Vec::new();

    for entry in WalkDir::new(directory_path) {
        let pathbuf: PathBuf = entry.unwrap().into_path().into();

        if pathbuf.is_dir().await {
            continue;
        }

        let printer = printer.clone();
        let matcher = matcher.clone();

        let task = async_std::task::spawn(async move {
            search_file(&pathbuf, matcher, printer).await;
        });

        spawned_tasks.push(task);
    }

    for task in spawned_tasks {
        task.await;
    }
}

async fn search_file<'a, M>(path: &Path, matcher: M, printer: ThreadedPrinterSender)
where
    M: Matcher + 'static,
{
    let file = File::open(path).await.expect("failed opening file");
    let file_size_bytes = fs::metadata(path)
        .await
        .expect("failed getting metadata")
        .len();
    let rdr = BufReader::new(file);

    let min_read_size = usize::min(file_size_bytes as usize + 512, MAX_BUFF_START_LEN);

    let line_buf = AsyncLineBufferBuilder::new()
        .with_minimum_read_size(min_read_size)
        .build();
    let line_buf_rdr = AsyncLineBufferReader::new(rdr, line_buf).line_nums(true);

    let target_name = Some(path.to_string_lossy().to_string());

    let status = search_via_reader(matcher, line_buf_rdr, target_name, printer).await;

    if let Err(e) = status {
        match e {
            // A binary file skip error is expected and can be ignored.
            Error::BinaryFileSkip(_) => {}
            _ => eprintln!("Unknown error while searching file: {:?}", e),
        }
    }
}

async fn search_via_reader<R, M>(
    matcher: M,
    mut buffer: AsyncLineBufferReader<R>,
    name: Option<String>,
    printer: ThreadedPrinterSender,
) -> Result<()>
where
    R: Read + std::marker::Unpin,
    M: Matcher,
{
    let mut binary_bytes_checked = 0;

    let name = name.unwrap_or_default();
    while let Some(line_result) = buffer.read_line().await {
        if binary_bytes_checked < BINARY_CHECK_LEN_BYTES {
            if check_utf8(line_result.text()) {
                binary_bytes_checked += line_result.text().len();
            } else {
                return Err(Error::BinaryFileSkip(name));
            }
        }

        if matcher.is_match(line_result.text()) {
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

    Ok(())
}

fn check_utf8(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok()
}
