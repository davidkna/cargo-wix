use anyhow::{bail, Result};
use structopt::StructOpt;
use fs_extra::dir::CopyOptions;

use std::env;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, StructOpt)]
enum XTask {
    Ghp,
}

fn main() -> Result<()> {
    let args = XTask::from_args();
    match args {
        XTask::Ghp => {
            let cargo = env::var("CARGO")
                .map(PathBuf::from)
                .ok()
                .unwrap_or_else(|| PathBuf::from("cargo"));
            let status = Command::new(cargo).arg("doc").arg("--no-deps").status()?;
            if !status.success() {
                bail!("The 'cargo doc' command failed");
            }
            let status = Command::new("git").arg("checkout").arg("gh-pages").status()?;
            if !status.success() {
                bail!("The 'git checkout gh-pages' command failed");
            }
            let mut target_doc_dir = PathBuf::from("target");
            target_doc_dir.push("doc");
            fs_extra::copy_items_with_progress(
                &target_doc_dir
                    .read_dir()?
                    .map(|entry| entry.unwrap().path())
                    .collect::<Vec<PathBuf>>(),
                &env::current_dir()?,
                &CopyOptions::new(),
                // &CopyOptions {
                //     overwrite: true,
                //     skip_exist: false,
                //     copy_inside: true,
                //     ..CopyOptions::new()
                // },
                |info| {
                    println!("{} in {}: {}", info.file_name, info.dir_name, info.total_bytes);
                    fs_extra::dir::TransitProcessResult::ContinueOrAbort
                }
            )?;
            let status = Command::new("git").arg("checkout").arg("main").status()?;
            if !status.success() {
                bail!("The 'git checkout main' command failed");
            }
        }
    }
    Ok(())
}
