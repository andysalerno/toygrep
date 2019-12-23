use crate::async_line_buffer::{AsyncLineBufferBuilder, AsyncLineBufferReader};
use crate::error::Result;
use crate::matcher::Matcher;
use crate::printer::PrintMessage;
use async_std::fs::{self, File};
use async_std::io::{BufReader, Read};
use async_std::path::Path;
use async_std::prelude::*;
use std::sync::mpsc::channel;
use std::sync::mpsc::Sender;

// Two megabyte max memory buffer len.
const MAX_BUFF_LEN_BYTES: usize = 2_000_000;

pub(crate) async fn search_via_reader<R, M>(
    matcher: M,
    mut buffer: AsyncLineBufferReader<R>,
    name: Option<String>,
    printer: Sender<PrintMessage>,
) -> Result<()>
where
    R: Read + std::marker::Unpin,
    M: Matcher,
{
    // TODO: fiddle with capacity
    let name = name.unwrap_or_default();
    while let Some(line_result) = buffer.read_line().await {
        if matcher.is_match(line_result.text()) {
            let printable = PrintMessage::PrintableResult {
                target_name: name.clone(),
                line_result,
            };
            printer.send(printable).expect("Failed sending to printer.");
        }
    }

    printer.send(PrintMessage::EndOfReading { target_name: name });

    drop(printer);

    Ok(())
}

pub(crate) async fn search_target<M: Matcher + 'static>(
    target_path: impl Into<&Path>,
    matcher: M,
    printer: Sender<PrintMessage>,
) {
    // If the target is a file, search it.
    let target_path = target_path.into();
    if target_path.is_file().await {
        search_file(target_path, matcher, printer).await;
    } else if target_path.is_dir().await {
        // If it's a directory, recurse into it and search all its contents.
        search_directory(target_path, matcher, printer).await;
    } else {
        panic!(
            "Couldn't find file or dir at path: {}. Btw, this should be an Err, not a panic...",
            target_path.display()
        );
    }
}

async fn search_directory<M: Matcher + 'static>(
    directory_path: &Path,
    matcher: M,
    printer: Sender<PrintMessage>,
) {
    let (sender, receiver) = channel();

    sender
        .send(directory_path.to_path_buf())
        .expect("Failure establishing sync channel.");

    let mut spawned_tasks = Vec::new();

    for dir_path in receiver.try_iter() {
        let mut dir_children = fs::read_dir(dir_path).await.expect("Failed to read dir.");

        while let Some(dir_child) = dir_children.next().await {
            let dir_child = dir_child.expect("Failed to make dir child.").path();

            if dir_child.is_file().await {
                let printer = printer.clone();
                let matcher = matcher.clone();

                let task = async_std::task::spawn(async move {
                    let dir_child_path: &Path = &dir_child;
                    search_file(dir_child_path, matcher, printer).await;
                });

                spawned_tasks.push(task);
            } else if dir_child.is_dir().await {
                sender
                    .send(dir_child)
                    .expect("Failure sending over sync channel.");
            }
        }
    }

    for task in spawned_tasks {
        let mut result = task.await;
    }
}

async fn search_file<M: Matcher>(
    // TODO: should be AsRef?
    file_path: impl Into<&Path>,
    matcher: M,
    printer: Sender<PrintMessage>,
) {
    let path = file_path.into();
    let file = File::open(path).await.expect("failed opening file");
    let file_size_bytes = fs::metadata(path)
        .await
        .expect("failed getting metadata")
        .len();
    let rdr = BufReader::new(file);

    let min_read_size = usize::min(file_size_bytes as usize + 512, MAX_BUFF_LEN_BYTES);

    let line_buf = AsyncLineBufferBuilder::new()
        .with_minimum_read_size(min_read_size)
        .build();
    let line_buf_rdr = AsyncLineBufferReader::new(rdr, line_buf);

    let target_name = Some(path.to_string_lossy().to_string());

    search_via_reader(matcher, line_buf_rdr, target_name, printer).await;
}
