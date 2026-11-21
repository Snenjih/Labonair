// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! Composition adapter for Explorer's remote-directory contract.
//!
//! The Explorer panel only needs a transport-neutral directory listing. The
//! shell owns the concrete SSH/SFTP services and adapts them at startup so the
//! panel does not depend directly on either remote capability.

use std::sync::Arc;

use labonair_explorer_host::{RemoteExplorerEntry, RemoteExplorerFuture, RemoteExplorerService};
use labonair_sftp::{SftpBrowserService, SftpSessionHandle, SftpSessionService};
use labonair_ssh::SshSessionId;
#[cfg(test)]
use std::future::Future;

pub(crate) struct SftpExplorerService {
    sessions: Arc<dyn SftpSessionService>,
    browser: Arc<dyn SftpBrowserService>,
}

impl SftpExplorerService {
    pub(crate) fn new(
        sessions: Arc<dyn SftpSessionService>,
        browser: Arc<dyn SftpBrowserService>,
    ) -> Self {
        Self { sessions, browser }
    }
}

impl RemoteExplorerService for SftpExplorerService {
    fn read_dir(
        &self,
        session_id: String,
        path: String,
    ) -> RemoteExplorerFuture<Vec<RemoteExplorerEntry>> {
        let sessions = self.sessions.clone();
        let browser = self.browser.clone();

        Box::pin(async move {
            let handle = sessions
                .open(SshSessionId::new(session_id))
                .await
                .map_err(|error| error.to_string())?;
            let entries = browser
                .read_dir(handle, path)
                .await
                .map_err(|error| error.to_string())?;

            Ok(entries
                .into_iter()
                .map(|entry| RemoteExplorerEntry {
                    name: entry.name,
                    path: entry.path,
                    size: entry.size,
                    modified_at: entry.modified_at,
                    is_dir: entry.is_dir,
                    is_symlink: entry.is_symlink,
                    symlink_target: entry.symlink_target,
                    permissions: entry.permissions,
                })
                .collect())
        })
    }

    fn create(&self, session_id: String, path: String, is_dir: bool) -> RemoteExplorerFuture<()> {
        let (sessions, browser) = (self.sessions.clone(), self.browser.clone());
        Box::pin(async move {
            let handle = open(&sessions, session_id).await?;
            if is_dir {
                browser.mkdir(handle, path, false).await
            } else {
                browser.create_file(handle, path).await
            }
            .map_err(|e| e.to_string())
        })
    }

    fn rename(&self, session_id: String, from: String, to: String) -> RemoteExplorerFuture<()> {
        let (sessions, browser) = (self.sessions.clone(), self.browser.clone());
        Box::pin(async move {
            let handle = open(&sessions, session_id).await?;
            browser
                .rename(handle, from, to)
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn delete(&self, session_id: String, path: String) -> RemoteExplorerFuture<()> {
        let (sessions, browser) = (self.sessions.clone(), self.browser.clone());
        Box::pin(async move {
            let handle = open(&sessions, session_id).await?;
            delete_recursive(&*browser, handle, path).await
        })
    }
}

async fn open(
    sessions: &Arc<dyn SftpSessionService>,
    session_id: String,
) -> Result<SftpSessionHandle, String> {
    sessions
        .open(SshSessionId::new(session_id))
        .await
        .map_err(|e| e.to_string())
}

/// SFTP `rmdir` only removes empty directories, so empty the tree first.
/// A directory is detected via `read_dir` of the parent (no symlink follow).
async fn delete_recursive(
    browser: &dyn SftpBrowserService,
    handle: SftpSessionHandle,
    path: String,
) -> Result<(), String> {
    let parent = path
        .rsplit_once('/')
        .map_or("/", |(p, _)| if p.is_empty() { "/" } else { p });
    let is_dir = browser
        .read_dir(handle.clone(), parent.to_string())
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .any(|e| e.path == path && e.is_dir && !e.is_symlink);
    if is_dir {
        let mut stack = vec![path.clone()];
        let mut dirs = Vec::new();
        while let Some(dir) = stack.pop() {
            let children = browser
                .read_dir(handle.clone(), dir.clone())
                .await
                .map_err(|e| e.to_string())?;
            dirs.push(dir);
            let (sub, files): (Vec<_>, Vec<_>) = children
                .into_iter()
                .partition(|e| e.is_dir && !e.is_symlink);
            if !files.is_empty() {
                browser
                    .delete(handle.clone(), files.into_iter().map(|e| e.path).collect())
                    .await
                    .map_err(|e| e.to_string())?;
            }
            stack.extend(sub.into_iter().map(|e| e.path));
        }
        // Deepest first: reverse discovery order empties children before parents.
        for dir in dirs.into_iter().rev() {
            browser
                .delete(handle.clone(), vec![dir])
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    } else {
        browser
            .delete(handle, vec![path])
            .await
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use labonair_errors::LabonairError;
    use labonair_sftp::{BoxFuture, RemoteEntry};
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// Fake tree: dir path -> children (name, is_dir). Records deletes.
    #[derive(Default)]
    struct Fake {
        tree: BTreeMap<String, Vec<(String, bool)>>,
        deleted: Mutex<Vec<String>>,
    }

    impl SftpBrowserService for Fake {
        fn read_dir<'a>(
            &'a self,
            _: SftpSessionHandle,
            path: String,
        ) -> BoxFuture<'a, Result<Vec<RemoteEntry>, LabonairError>> {
            let kids = self.tree.get(&path).cloned().unwrap_or_default();
            Box::pin(async move {
                Ok(kids
                    .into_iter()
                    .map(|(n, d)| RemoteEntry {
                        path: format!("{}/{n}", path.trim_end_matches('/')),
                        name: n,
                        size: 0,
                        modified_at: 0,
                        is_dir: d,
                        is_symlink: false,
                        symlink_target: None,
                        permissions: String::new(),
                    })
                    .collect())
            })
        }
        fn delete<'a>(
            &'a self,
            _: SftpSessionHandle,
            paths: Vec<String>,
        ) -> BoxFuture<'a, Result<(), LabonairError>> {
            self.deleted.lock().unwrap().extend(paths);
            Box::pin(async { Ok(()) })
        }
        fn rename<'a>(
            &'a self,
            _: SftpSessionHandle,
            _: String,
            _: String,
        ) -> BoxFuture<'a, Result<(), LabonairError>> {
            unimplemented!()
        }
        fn mkdir<'a>(
            &'a self,
            _: SftpSessionHandle,
            _: String,
            _: bool,
        ) -> BoxFuture<'a, Result<(), LabonairError>> {
            unimplemented!()
        }
        fn create_file<'a>(
            &'a self,
            _: SftpSessionHandle,
            _: String,
        ) -> BoxFuture<'a, Result<(), LabonairError>> {
            unimplemented!()
        }
        fn chmod<'a>(
            &'a self,
            _: SftpSessionHandle,
            _: String,
            _: u32,
        ) -> BoxFuture<'a, Result<(), LabonairError>> {
            unimplemented!()
        }
    }

    #[test]
    fn deletes_directory_children_before_parents() {
        let mut fake = Fake::default();
        fake.tree.insert("/".into(), vec![("d".into(), true)]);
        fake.tree
            .insert("/d".into(), vec![("f".into(), false), ("s".into(), true)]);
        fake.tree.insert("/d/s".into(), vec![("g".into(), false)]);
        let handle = SftpSessionHandle::from_ssh_session(SshSessionId::new("x"));
        futures_lite_block(delete_recursive(&fake, handle, "/d".into())).unwrap();
        assert_eq!(
            *fake.deleted.lock().unwrap(),
            vec!["/d/f", "/d/s/g", "/d/s", "/d"]
        );
    }

    fn futures_lite_block<T>(f: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }
}
