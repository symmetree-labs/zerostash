use std::{
    collections::VecDeque,
    sync::{atomic::AtomicUsize, Arc},
};

use infinitree::{
    fields::{Collection, Store, VersionedMap},
    Digest,
};

use crate::Entry;

#[derive(Debug)]
pub enum FsError<'a> {
    InvalidPath(Vec<&'a str>),
    NoSuchFileOrDirectory,
    InvalidFilesystem,
}
pub type Result<'a, T> = std::result::Result<T, FsError<'a>>;

type InnerTree = VersionedMap<Digest, Node>;

#[derive(Default)]
pub struct Tree(pub InnerTree);

// auto-derive will not work for resolving constraints properly
impl Clone for Tree {
    fn clone(&self) -> Self {
        Tree(self.0.clone())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Node {
    File {
        refs: Arc<AtomicUsize>,
        entry: Arc<Entry>,
    },
    Directory {
        entries: scc::HashMap<String, Digest>,
    },
}

impl Node {
    fn directory() -> Node {
        Node::Directory {
            entries: scc::HashMap::with_capacity(0),
        }
    }

    fn file(entry: Entry) -> Node {
        Node::File {
            refs: AtomicUsize::new(1).into(),
            entry: entry.into(),
        }
    }

    pub fn as_file(&self) -> Option<Arc<Entry>> {
        match self {
            Node::File { entry, .. } => Some(Arc::clone(entry)),
            _ => None,
        }
    }

    pub fn is_file(&self) -> bool {
        matches!(self, Node::File { .. })
    }

    pub fn is_dir(&self) -> bool {
        matches!(self, Node::Directory { .. })
    }
}

impl Tree {
    pub fn insert_root<'a>(&self) -> Result<'a, ()> {
        if self.0.get(&Digest::default()).is_none() {
            _ = self.0.insert(Digest::default(), Node::directory());
        }
        Ok(())
    }

    /// Create a new directory at `path`, creating all entries in between
    pub fn insert_directory<'a>(&self, path: &'a str) -> Result<'a, ()> {
        self.insert_root().unwrap();
        let (noderef, _current, filename) = self.create_path_to_parent(path)?;
        self.add_empty_dir(&noderef, filename);
        Ok(())
    }

    /// Insert or overwrite an file at `path`, creating all entries in between
    pub fn insert_file<'a>(&self, path: &'a str, file: Entry) -> Result<'a, ()> {
        self.insert_root().unwrap();
        let (noderef, _current, filename) = self.create_path_to_parent(path)?;
        self.add_file(&noderef, filename, file);
        Ok(())
    }

    pub fn update_file<'a>(&self, path: &'a str, file: Entry) -> Result<'a, ()> {
        self.insert_root().unwrap();
        let file_ref = self.get_ref(path)?.ok_or(FsError::NoSuchFileOrDirectory)?;
        self.0
            .update_with(file_ref, |_| Node::file(file))
            .ok_or(FsError::InvalidFilesystem)?;
        Ok(())
    }

    /// Recursively remove a subtree the `path`
    pub fn remove<'a>(&self, path: &'a str) -> Result<'a, ()> {
        self.insert_root().unwrap();
        let (parent_ref, parent, to_delete) = self.path_to_parent(path)?;
        let stack = scc::Stack::default();

        if let Node::Directory { ref entries } = parent.as_ref() {
            let file_node_ref = entries
                .read(to_delete, |_, v| *v)
                .ok_or(FsError::NoSuchFileOrDirectory)?;

            stack.push((parent_ref, file_node_ref, to_delete.to_string()));

            while let Some(next) = stack.pop().as_deref() {
                let (parent_ref, noderef, filename) = (next.0, next.1, next.2.to_string());
                let file_node = self.0.get(&noderef);

                match file_node.as_deref() {
                    Some(Node::File { .. }) => {
                        self.remove_file(&parent_ref, &filename);
                        self.0.remove(file_node_ref);
                        entries.remove(&filename);
                    }

                    Some(Node::Directory {
                        entries: ref inner_entries,
                    }) => {
                        if inner_entries.is_empty() {
                            // if it's empty, just delete it
                            //
                            self.remove_file(&parent_ref, &filename);
                            self.0.remove(noderef);
                            entries.remove(&filename);
                        } else {
                            // if not empty, queue it for visit again
                            //
                            stack.push((parent_ref, noderef, filename.to_string()));

                            // then push all the children as well
                            //
                            inner_entries.scan(|file, childref| {
                                stack.push((noderef, *childref, file.to_string()));
                            });
                        }
                    }

                    // technically this should not happen, maybe error
                    // reporting could be better
                    None => return Err(FsError::InvalidFilesystem),
                }
            }
        }

        Ok(())
    }

    /// Return a file
    pub fn get_file<'a>(&self, path: &'a str) -> Result<'a, Option<Arc<Entry>>> {
        self.insert_root().unwrap();
        let Some(noderef) = self.get_ref(path)? else {
            return Ok(None);
        };

        let Some(node) = self.0.get(&noderef) else {
            return Err(FsError::InvalidFilesystem);
        };

        Ok(node.as_file())
    }

    /// Return a file system node
    pub fn get<'a>(&self, path: &'a str) -> Result<'a, Option<Arc<Node>>> {
        self.insert_root().unwrap();
        let Some(noderef) = self.get_ref(path)? else {
            return Ok(None);
        };

        let Some(node) = self.0.get(&noderef) else {
            return Err(FsError::InvalidFilesystem);
        };

        Ok(Some(node))
    }

    /// Move the file from the old path to the new path in the tree
    pub fn move_node<'a>(&self, old_path: &'a str, new_path: &'a str) -> Result<'a, ()> {
        self.insert_root().unwrap();
        let (parent_ref, _, node_name) = self.path_to_parent(old_path)?;
        let noderef = {
            let mut noderef = None;
            self.0.update_with(parent_ref, |v| {
                let current = v.as_ref().clone();
                let Node::Directory { ref entries } = current else {
                    unreachable!()
                };
                noderef = entries.remove(node_name);
                current
            });

            let Some((_, noderef)) = noderef else {
                return Err(FsError::NoSuchFileOrDirectory);
            };
            noderef
        };

        // add to the new place, overwriting an existing file there
        let (new_ref, _, new_node_name) = self.path_to_parent(new_path)?;
        self.0.update_with(new_ref, |v| {
            let new = v.as_ref().clone();
            let Node::Directory { ref entries } = new else {
                unreachable!()
            };
            _ = entries.insert(new_node_name.into(), noderef);
            new
        });

        Ok(())
    }

    pub fn root(&self) -> Arc<Node> {
        self.0.get(&Digest::default()).expect("uninitialized tree")
    }

    fn remove_file(&self, parent_ref: &Digest, filename: &str) {
        self.0.update_with(*parent_ref, |v| {
            let new = v.as_ref().clone();
            if let Node::Directory { ref entries } = new {
                entries.remove(filename);
                return new;
            }
            unreachable!()
        });
    }

    /// Return the internal reference to the path
    fn get_ref<'a>(&self, path: &'a str) -> Result<'a, Option<Digest>> {
        let (_, current, filename) = self.path_to_parent(path)?;
        let Node::Directory { ref entries } = current.as_ref() else {
            unreachable!()
        };

        Ok(entries.read(filename, |_, v| *v))
    }

    /// Returns if the path is a file
    pub fn is_file<'a>(&self, path: &'a str) -> Result<'a, bool> {
        self.insert_root().unwrap();
        let (_, current, _filename) = self.path_to_parent(path)?;

        let Node::Directory { ref entries } = current.as_ref() else {
            unreachable!()
        };

        Ok(entries
            .read(path, |_, v| *v)
            .and_then(|key| self.0.get(&key))
            .map(|node| node.is_file())
            .unwrap_or(false))
    }

    /// Traverses the tree to the parent node of the path
    ///
    /// # Errors
    ///
    /// Returns a list of valid path components, followed by the
    /// erroring component.
    fn path_to_parent<'a>(&self, path: &'a str) -> Result<'a, (Digest, Arc<Node>, &'a str)> {
        let parts = path
            .split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<&str>>();

        let mut current = Some(self.root());
        let mut current_ref = Some(Digest::default());
        let mut consumed = vec![];

        let take_len = if parts.is_empty() { 0 } else { parts.len() - 1 };
        for part in parts.iter().take(take_len) {
            consumed.push(*part);

            let Some(Node::Directory { entries }) = current.as_deref() else {
                return Err(FsError::InvalidPath(consumed));
            };

            current_ref = entries.read(*part, |_, v| *v);
            current = current_ref.and_then(|nodeid| self.0.get(&nodeid));
        }

        let parent = current.ok_or(FsError::InvalidPath(consumed))?;
        Ok((
            current_ref.expect("must be Some"),
            parent,
            parts.last().unwrap_or(&""),
        ))
    }

    /// Traverses the tree to the parent node of the path, or creates
    /// the entire path
    ///
    /// # Errors
    ///
    /// Returns a list of valid path components, followed by the
    /// erroring component.
    fn create_path_to_parent<'a>(&self, path: &'a str) -> Result<'a, (Digest, Arc<Node>, &'a str)> {
        let parts = path
            .split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<&str>>();

        let mut parent: Digest;
        let mut current = Some(self.root());
        let mut current_ref = Some(Digest::default());
        let mut consumed = vec![];

        let take_len = if parts.is_empty() { 0 } else { parts.len() - 1 };
        for part in parts.iter().take(take_len) {
            consumed.push(*part);

            let Some(Node::Directory { entries }) = current.as_deref() else {
                return Err(FsError::InvalidPath(consumed));
            };

            if let Some(new_current_ref) = entries.read(*part, |_, v| *v) {
                current_ref = Some(new_current_ref);
                current = current_ref.and_then(|noderef| self.0.get(&noderef));
            } else {
                parent = current_ref.expect("current is Some");
                let (new_current_ref, new_current) = self.add_empty_dir(&parent, part);
                current_ref = Some(new_current_ref);
                current = Some(new_current);
            }
        }

        Ok((
            current_ref.expect("must be Some"),
            current.expect("must be Some"),
            parts.last().unwrap_or(&""),
        ))
    }

    fn add_empty_dir(&self, parent: &Digest, name: &str) -> (Digest, Arc<Node>) {
        let noderef: Digest = rand::random();
        self.0.update_with(*parent, |parent| {
            let Node::Directory { ref entries } = parent.as_ref() else {
                panic!("invalid use of library");
            };
            _ = entries.insert(name.into(), noderef);
            parent.as_ref().clone()
        });

        self.0.insert(noderef, Node::directory());
        (noderef, self.0.get(&noderef).expect("just inserted"))
    }

    fn add_file(&self, parent: &Digest, name: &str, file: Entry) -> (Digest, Arc<Node>) {
        let noderef: Digest = rand::random();
        self.0.update_with(*parent, |parent| {
            let Node::Directory { ref entries } = parent.as_ref() else {
                panic!("invalid use of library");
            };
            _ = entries.insert(name.into(), noderef);
            parent.as_ref().clone()
        });

        self.0.insert(noderef, Node::file(file));
        (noderef, self.0.get(&noderef).expect("just inserted"))
    }

    pub fn retain<F>(&self, mut f: F)
    where
        F: FnMut(&str, &Node) -> bool,
    {
        self.insert_root().unwrap();
        let entries = match self.root().as_ref() {
            Node::Directory { entries } => entries.clone(),
            Node::File { refs: _, entry: _ } => panic!(""),
        };

        let mut stack = VecDeque::new();
        stack.push_front((String::new(), entries));
        let mut to_remove = vec![];

        while let Some((path, node)) = stack.pop_front() {
            node.scan(|relative_path, noderef| {
                let full_path = if path.is_empty() {
                    relative_path.clone()
                } else {
                    format!("{}/{}", path, relative_path)
                };

                if let Some(node) = self.0.get(noderef) {
                    match node.as_ref() {
                        Node::File { refs: _, entry: _ } => {
                            if !f(&full_path, &node) {
                                to_remove.push(full_path);
                            }
                        }
                        Node::Directory { entries } => {
                            //issue with removal of folders so currently only files
                            stack.push_front((full_path, entries.clone()));
                        }
                    }
                }
            })
        }

        for key in to_remove {
            let _ = self.remove(&key);
        }
    }

    pub fn iter_files(&self) -> TreeIterator {
        self.insert_root().unwrap();
        let root = Arc::clone(&self.root());
        let stack = vec![(String::new(), root)];
        TreeIterator { stack, inner: self }
    }
}

pub struct TreeIterator<'a> {
    stack: Vec<(String, Arc<Node>)>,
    inner: &'a Tree,
}

impl<'a> Iterator for TreeIterator<'a> {
    type Item = (String, Arc<Entry>);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((prefix, node)) = self.stack.pop() {
            match node.as_ref() {
                Node::File { refs: _, entry } => return Some((prefix, entry.clone())),
                Node::Directory { entries } => {
                    entries.scan(|name, digest| {
                        let path = if prefix.is_empty() {
                            name.to_string()
                        } else {
                            format!("{prefix}/{name}")
                        };
                        if let Some(curr) = self.inner.0.get(digest) {
                            self.stack.push((path, curr));
                        }
                    });
                }
            }
        }
        None
    }
}

impl Collection for Tree {
    type Depth = infinitree::fields::depth::Incremental;

    type Key = <InnerTree as Collection>::Key;

    type Serialized = <InnerTree as Collection>::Serialized;

    type Item = <InnerTree as Collection>::Item;

    fn key(from: &Self::Serialized) -> &Self::Key {
        <InnerTree as Collection>::key(from)
    }

    fn load(from: Self::Serialized, object: &mut dyn infinitree::object::Reader) -> Self::Item {
        <InnerTree as Collection>::load(from, object)
    }

    fn insert(&mut self, record: Self::Item) {
        <InnerTree as Collection>::insert(&mut self.0, record)
    }
}

impl Store for Tree {
    fn store(
        &mut self,
        transaction: &mut dyn infinitree::index::Transaction,
        object: &mut dyn infinitree::object::Writer,
    ) {
        <InnerTree as Store>::store(&mut self.0, transaction, object)
    }
}

#[cfg(test)]
mod test {
    use infinitree::{crypto::UsernamePassword, Infinitree};

    use crate::{Entry, Files, Tree};

    #[test]
    fn create_path_to_parent() {
        let tree = Tree::default();
        tree.insert_root().unwrap();
        let path = "/test/path/to/dir";

        assert!(tree.path_to_parent(path).is_err());
        let res = tree.create_path_to_parent(path);
        assert!(res.is_ok());

        let (_, _, dir) = res.unwrap();
        assert_eq!("dir", dir);

        assert!(tree.path_to_parent(path).is_ok());
    }

    #[test]
    fn test_file_insert_and_removal() {
        let tree = Tree::default();
        let file_path = "test/path/to/file.rs";

        let res = tree.insert_file(file_path, Entry::default());
        assert!(res.is_ok());

        let res = tree.get_file(file_path);
        assert!(res.is_ok() && res.unwrap().is_some());

        assert!(tree.remove(file_path).is_ok());
    }

    #[test]
    fn test_file_iteration() {
        let tree = Tree::default();
        let file1 = "test/path/to/file1.rs".to_string();
        let file2 = "test/path/to/file2.rs".to_string();

        let files = vec![file1.clone(), file2.clone()];

        assert!(tree.insert_file(&file1, Entry::default()).is_ok());
        assert!(tree.insert_file(&file2, Entry::default()).is_ok());

        for (k, _) in tree.iter_files() {
            assert!(files.contains(&k));
        }
    }

    #[test]
    fn bare_index_can_be_restored() {
        let key = || {
            UsernamePassword::with_credentials("bare_index_map".to_string(), "password".to_string())
                .unwrap()
        };
        let storage = crate::backends::test::InMemoryBackend::shared();

        {
            let mut tree = Infinitree::<Files>::empty(storage.clone(), key()).unwrap();

            {
                let index = &tree.index().tree;
                let file1 = "file1.rs".to_string();
                let file2 = "file2.rs".to_string();

                index.insert_file(&file1, Entry::default()).unwrap();
                index.insert_file(&file2, Entry::default()).unwrap();
            }

            assert_eq!(tree.index().tree.0.len(), 3);
            tree.commit(None).unwrap();
            tree.index().tree.0.clear();
            tree.load_all().unwrap();

            {
                let index = &tree.index().tree;
                let _ = index.insert_root();
                let file3 = "file3.rs".to_string();
                index.insert_file(&file3, Entry::default()).unwrap();
            }

            tree.commit(None).unwrap();
        }

        let tree = Infinitree::<Files>::open(storage, key()).unwrap();
        tree.load_all().unwrap();

        assert_eq!(tree.index().tree.0.len(), 4);
    }

    #[test]
    fn test_iter() {
        let key = || {
            UsernamePassword::with_credentials("bare_index_map".to_string(), "password".to_string())
                .unwrap()
        };
        let storage = crate::backends::test::InMemoryBackend::shared();
        let file1 = "test/path/to/file1.rs".to_string();
        let file2 = "test/path/to/file2.rs".to_string();
        let file3 = "test/path/to/file3.rs".to_string();

        {
            let mut tree = Infinitree::<Files>::empty(storage.clone(), key()).unwrap();

            {
                let _ = tree.index().tree.insert_file(&file1, Entry::default());
                let _ = tree.index().tree.insert_file(&file2, Entry::default());
                assert_eq!(tree.index().tree.iter_files().count(), 2);
            }

            tree.commit(None).unwrap();
            tree.index().tree.0.clear();
            tree.load_all().unwrap();

            {
                let _ = tree.index().tree.insert_file(&file3, Entry::default());
                assert_eq!(tree.index().tree.iter_files().count(), 3);
            }

            tree.commit(None).unwrap();
        }

        let tree = Infinitree::<Files>::open(storage, key()).unwrap();
        tree.load_all().unwrap();
        assert_eq!(tree.index().tree.iter_files().count(), 3);
    }

    #[test]
    fn test_dir_removal() {
        let tree = Tree::default();
        let file1 = "test/path/to/file1.rs".to_string();
        let file2 = "test/path/to/file2.rs".to_string();
        let file3 = "test/path/to/file3.rs".to_string();
        let files = [file1, file2, file3];

        let _ = tree.insert_file(&files[0], Entry::default());
        let _ = tree.insert_file(&files[1], Entry::default());
        let _ = tree.insert_file(&files[2], Entry::default());

        for file in files.iter() {
            let file = tree.get_file(file);
            assert!(file.is_ok() & file.unwrap().is_some());
        }

        let _ = tree.remove("test/path/to");
        let folder = tree.get_file("test/path/to");
        assert!(folder.is_err());

        for file in files.iter() {
            let file = tree.get_file(file);
            assert!(file.is_err());
        }
    }

    #[test]
    fn path_to_root() {
        let tree = Tree::default();
        let _ = tree.insert_root();

        let res = tree.path_to_parent("/");
        assert!(res.is_ok());
    }
}
