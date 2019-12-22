use crate::async_line_buffer::{AsyncLineBufferBuilder, AsyncLineBufferReader};
use crate::error::Result;
use crate::printer::PrintableResult;
use async_std::fs::{self, File};
use async_std::io::{BufReader, Read};
use async_std::path::Path;
use async_std::prelude::*;
use regex::Regex;
use std::sync::mpsc::channel;
use std::sync::mpsc::Sender;

// Two megabyte max memory buffer len.
const MAX_BUFF_LEN_BYTES: usize = 2_000_000;

pub(crate) async fn search_via_reader<R>(
    pattern: &Regex,
    mut buffer: AsyncLineBufferReader<R>,
    name: Option<String>,
    printer: Sender<PrintableResult>,
) -> Result<()>
where
    R: Read + std::marker::Unpin,
{
    // TODO: fiddle with capacity
    let name = name.unwrap_or_default();
    while let Some(line_bytes) = buffer.read_line().await {
        let line_result = line_bytes?;
        if pattern.is_match(line_result.text()) {
            let printable = PrintableResult {
                target_name: name.clone(),
                line_result,
            };
            printer.send(printable).expect("Failed sending to printer.");
        }
    }

    drop(printer);

    Ok(())
}

pub(crate) async fn search_target(
    target_path: impl Into<&Path>,
    pattern: &Regex,
    printer: Sender<PrintableResult>,
) {
    // If the target is a file, search it.
    let target_path = target_path.into();
    if target_path.is_file().await {
        search_file(target_path, pattern, printer).await;
    } else if target_path.is_dir().await {
        // If it's a directory, recurse into it and search all its contents.
        search_directory(target_path, pattern, printer).await;
    } else {
        panic!(
            "Couldn't find file or dir at path: {}. Btw, this should be an Err, not a panic...",
            target_path.display()
        );
    }
}

async fn search_directory(
    directory_path: &Path,
    pattern: &Regex,
    printer: Sender<PrintableResult>,
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
            let pattern = pattern.clone();

            let printer = printer.clone();

            if dir_child.is_file().await {
                let task = async_std::task::spawn(async move {
                    let dir_child_path: &Path = &dir_child;

                    search_file(dir_child_path, &pattern, printer).await;
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

async fn search_file(
    // TODO: should be AsRef?
    file_path: impl Into<&Path>,
    pattern: &Regex,
    printer: Sender<PrintableResult>,
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

    search_via_reader(pattern, line_buf_rdr, target_name, printer).await;
}
