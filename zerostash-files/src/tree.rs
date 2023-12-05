use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    vec,
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

// InnerTree, is root initialized
pub struct Tree(InnerTree, AtomicBool);

impl Default for Tree {
    fn default() -> Tree {
        let tree = Tree(InnerTree::default(), false.into());
        tree.insert_root().unwrap();
        tree
    }
}

// auto-derive will not work for resolving constraints properly
impl Clone for Tree {
    fn clone(&self) -> Self {
        Tree(self.0.clone(), self.1.load(Ordering::SeqCst).into())
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
    fn insert_root<'a>(&self) -> Result<'a, ()> {
        if self.0.get(&Digest::default()).is_none() {
            _ = self.0.insert(Digest::default(), Node::directory());
        }
        self.1.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Create a new directory at `path`, creating all entries in between
    pub fn insert_directory<'a>(&self, path: &'a str) -> Result<'a, ()> {
        let (noderef, _current, filename) = self.create_path_to_parent(path)?;
        self.add_empty_dir(&noderef, filename);
        Ok(())
    }

    /// Insert or overwrite an file at `path`, creating all entries in between
    pub fn insert_file<'a>(&self, path: &'a str, file: Entry) -> Result<'a, ()> {
        let (noderef, _current, filename) = self.create_path_to_parent(path)?;
        self.add_file(&noderef, filename, file);
        Ok(())
    }

    /// Updates an file at `path`
    pub fn update_file<'a>(&self, path: &'a str, file: Entry) -> Result<'a, ()> {
        let file_ref = self.get_ref(path)?.ok_or(FsError::NoSuchFileOrDirectory)?;
        self.0
            .update_with(file_ref, |_| Node::file(file))
            .ok_or(FsError::InvalidFilesystem)?;
        Ok(())
    }

    /// Recursively remove a subtree the `path`
    pub fn remove<'a>(&self, path: &'a str) -> Result<'a, ()> {
        let (parent_ref, parent, to_delete) = self.path_to_parent(path)?;

        let stack = scc::Stack::default();

        if let Node::Directory { ref entries } = parent.as_ref() {
            let node_ref = entries
                .read(to_delete, |_, v| *v)
                .ok_or(FsError::NoSuchFileOrDirectory)?;

            stack.push((parent_ref, node_ref, to_delete.to_string()));

            while let Some(next) = stack.pop().as_deref() {
                let (parent_ref, node_ref, entry_name) = (next.0, next.1, next.2.to_string());
                let file_node = self.0.get(&node_ref);

                match file_node.as_deref() {
                    Some(Node::File { .. }) => {
                        self.remove_file(&parent_ref, &entry_name);
                        self.0.remove(node_ref);
                    }

                    Some(Node::Directory {
                        entries: ref inner_entries,
                    }) => {
                        if inner_entries.is_empty() {
                            // if it's empty, just delete it
                            //
                            self.remove_file(&parent_ref, &entry_name);
                            self.0.remove(node_ref);
                        } else {
                            // if not empty, queue it for visit again
                            //
                            stack.push((parent_ref, node_ref, entry_name.to_string()));

                            // then push all the children as well
                            //
                            inner_entries.scan(|file, childref| {
                                stack.push((node_ref, *childref, file.to_string()));
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
    pub fn file<'a>(&self, path: &'a str) -> Result<'a, Option<Arc<Entry>>> {
        let Some(noderef) = self.get_ref(path)? else {
            return Ok(None);
        };

        let Some(node) = self.0.get(&noderef) else {
            return Err(FsError::InvalidFilesystem);
        };

        Ok(node.as_file())
    }

    pub fn node_by_ref(&self, noderef: &Digest) -> Option<Arc<Node>> {
        self.0.get(noderef)
    }

    /// Return a file system node
    pub fn node_by_path<'a>(&self, path: &'a str) -> Result<'a, Option<Arc<Node>>> {
        if path == "/" {
            return Ok(Some(self.root()));
        }

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
        let (parent_ref, _, node_name) = self.path_to_parent(old_path)?;
        let noderef = {
            let mut noderef = None;
            self.0.update_with(parent_ref, |current| {
                let Node::Directory { ref entries } = current.as_ref() else {
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
        self.0.update_with(new_ref, |new| {
            let Node::Directory { ref entries } = new.as_ref() else {
                unreachable!()
            };
            _ = entries.insert(new_node_name.into(), noderef);
            new
        });

        Ok(())
    }

    fn root(&self) -> Arc<Node> {
        self.0.get(&Digest::default()).unwrap()
    }

    fn remove_file(&self, parent_ref: &Digest, filename: &str) {
        self.0.update_with(*parent_ref, |new| {
            if let Node::Directory { ref entries } = new.as_ref() {
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
    /// Returns a list of valid path components, followed by the
    /// erroring component.
    fn path_to_parent<'a>(&self, path: &'a str) -> Result<'a, (Digest, Arc<Node>, &'a str)> {
        let mut parts = path
            .split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<&'a str>>();

        if parts.is_empty() {
            return Err(FsError::InvalidPath(Vec::default()));
        }

        let mut current = Some(self.root());
        let mut current_ref = Some(Digest::default());
        let mut consumed = Vec::with_capacity(parts.len());
        let last_part = parts.pop().unwrap();

        for part in parts.iter() {
            consumed.push(*part);

            let Some(Node::Directory { entries }) = current.as_deref() else {
                return Err(FsError::InvalidPath(consumed));
            };

            current_ref = entries.read(*part, |_, v| *v);
            current = current_ref.and_then(|nodeid| self.0.get(&nodeid));
        }

        let (parent, current_ref) = current
            .zip(current_ref)
            .ok_or(FsError::InvalidPath(consumed))?;

        Ok((current_ref, parent, last_part))
    }

    /// Traverses the tree to the parent node of the path, or creates
    /// the entire path
    ///
    /// Returns a list of valid path components, followed by the
    /// erroring component.
    fn create_path_to_parent<'a>(&self, path: &'a str) -> Result<'a, (Digest, Arc<Node>, &'a str)> {
        let mut parts = path
            .split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<&'a str>>();

        if parts.is_empty() {
            return Err(FsError::InvalidPath(Vec::default()));
        }

        let mut parent: Digest;
        let mut current = Some(self.root());
        let mut current_ref = Some(Digest::default());
        let mut consumed = Vec::with_capacity(parts.len());
        let last_part = parts.pop().unwrap();

        for part in parts.iter() {
            consumed.push(*part);

            let Some(Node::Directory { entries }) = current.as_deref() else {
                return Err(FsError::InvalidPath(consumed));
            };

            if let Some(new_current_ref) = entries.read(*part, |_, v| *v) {
                current_ref = Some(new_current_ref);
                current = current_ref.and_then(|noderef| self.0.get(&noderef));
            } else if let Some(parent_ref) = current_ref {
                parent = parent_ref;
                let (new_current_ref, new_current) = self.add_empty_dir(&parent, part);
                current_ref = Some(new_current_ref);
                current = Some(new_current);
            } else {
                return Err(FsError::InvalidPath(consumed));
            }
        }

        let (parent, current_ref) = current
            .zip(current_ref)
            .ok_or(FsError::InvalidPath(consumed))?;

        Ok((current_ref, parent, last_part))
    }

    fn add_empty_dir(&self, parent: &Digest, name: &str) -> (Digest, Arc<Node>) {
        let noderef: Digest = rand::random();
        self.0.update_with(*parent, |parent| {
            let Node::Directory { ref entries } = parent.as_ref() else {
                panic!("invalid use of library");
            };
            _ = entries.insert(name.into(), noderef);
            parent
        });

        self.0.insert(noderef, Node::directory());
        (noderef, self.0.get(&noderef).unwrap())
    }

    fn add_file(&self, parent: &Digest, name: &str, file: Entry) -> (Digest, Arc<Node>) {
        let mut noderef: Digest = rand::random();
        let mut update = false;
        self.0.update_with(*parent, |parent| {
            let Node::Directory { ref entries } = parent.as_ref() else {
                panic!("invalid use of library");
            };
            match entries.entry(name.into()) {
                scc::hash_map::Entry::Occupied(mut entry) => {
                    noderef = *entry.get_mut();
                    update = true;
                }
                scc::hash_map::Entry::Vacant(entry) => {
                    entry.insert_entry(noderef);
                }
            }
            parent
        });

        let new_node = Arc::new(Node::file(file));
        if update {
            self.0.update_with(noderef, |_| new_node.clone());
        } else {
            self.0.insert(noderef, new_node.clone());
        }
        (noderef, new_node)
    }

    pub fn retain<F>(&self, mut f: F)
    where
        F: FnMut(&str, &Node) -> bool,
    {
        let stack = scc::Stack::default();
        stack.push((String::new(), Digest::default()));

        let mut to_remove = vec![];

        while let Some(next) = stack.pop() {
            let (path, noderef) = (next.0.to_string(), next.1);
            if let Some(node) = self.0.get(&noderef) {
                match node.as_ref() {
                    Node::File { refs: _, entry: _ } => {
                        if !f(&path, &node) {
                            to_remove.push(path);
                        }
                    }
                    Node::Directory { entries } => {
                        if !f(&path, &node) {
                            to_remove.push(path);
                        } else {
                            entries.scan(|relative_path, noderef| {
                                let full_path = if path.is_empty() {
                                    relative_path.clone()
                                } else {
                                    format!("{}/{}", path, relative_path)
                                };
                                stack.push((full_path, *noderef));
                            });
                        }
                    }
                }
            }
        }

        for key in to_remove {
            _ = self.remove(&key);
        }
    }

    pub fn iter_files(&self) -> TreeIterator {
        let root = Arc::clone(&self.root());
        let stack = scc::Stack::default();
        stack.push((String::new(), root));
        TreeIterator { stack, inner: self }
    }

    pub fn clear(&self) -> Result<'_, ()> {
        self.0.clear();
        self.insert_root()
    }
}

pub struct TreeIterator<'a> {
    stack: scc::Stack<(String, Arc<Node>)>,
    inner: &'a Tree,
}

impl<'a> Iterator for TreeIterator<'a> {
    type Item = (String, Arc<Entry>);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(next) = self.stack.pop() {
            let (prefix, node) = (next.0.to_string(), next.1.as_ref());
            match node {
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
        static ROOT: Digest = [0u8; 32];
        let (key, new_entry) = record;

        if !self.1.load(Ordering::Relaxed) && key == ROOT && self.0.contains(&ROOT) {
            self.0.update_with(key, |_| new_entry.unwrap());
            self.1.store(true, Ordering::Relaxed);
        } else if !self.0.contains(&key) {
            <InnerTree as Collection>::insert(&mut self.0, (key, new_entry))
        }
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
    use std::assert_eq;

    use infinitree::{crypto::UsernamePassword, Digest, Infinitree};
    use scc::HashSet;

    use crate::{Entry, Files, Node, Tree};

    #[test]
    fn test_create_path_to_parent() {
        let tree = Tree::default();
        let path = "/test/path/to/dir";

        assert!(tree.path_to_parent(path).is_err());
        let res = tree.create_path_to_parent(path);
        assert!(res.is_ok());

        let (_, _, dir) = res.unwrap();
        assert_eq!("dir", dir);

        assert!(tree.path_to_parent(path).is_ok());
    }

    #[test]
    fn test_insert_and_remove_file() {
        let tree = Tree::default();
        let file_path = "test/path/to/file.rs";

        let res = tree.insert_file(file_path, Entry::default());
        assert!(res.is_ok());

        let res = tree.file(file_path);
        assert!(res.is_ok() && res.unwrap().is_some());

        assert!(tree.remove(file_path).is_ok());
        assert!(tree.file(file_path).unwrap().is_none());
    }

    #[test]
    fn test_move_node() {
        let key = || {
            UsernamePassword::with_credentials("bare_index_map".to_string(), "password".to_string())
                .unwrap()
        };
        let storage = crate::backends::test::InMemoryBackend::shared();
        let file_path = "test/path/file.rs".to_string();
        let random_file = "test/path/to/random.rs".to_string();
        let new_file_path = "test/path/to/file.rs".to_string();
        let entry = Entry {
            name: String::from("file.rs"),
            size: 1234,
            ..Default::default()
        };

        {
            let mut tree = Infinitree::<Files>::empty(storage.clone(), key()).unwrap();

            {
                let tree_index = &tree.index().tree;
                _ = tree_index.insert_file(&file_path, entry.clone());
                _ = tree_index.insert_file(&random_file, Entry::default());

                _ = tree_index.move_node(&file_path, &new_file_path);
                assert!(tree_index.file(&file_path).unwrap().is_none());

                assert_eq!(
                    tree_index.file(&new_file_path).unwrap().unwrap().as_ref(),
                    &entry
                );
            }

            tree.commit(None).unwrap();
            tree.index().tree.clear();
            tree.load_all().unwrap();

            {
                let tree_index = &tree.index().tree;

                assert!(tree_index.file(&file_path).unwrap().is_none());
                assert_eq!(
                    tree_index.file(&new_file_path).unwrap().unwrap().as_ref(),
                    &entry
                );
            }

            tree.commit(None).unwrap();
        }

        let tree = Infinitree::<Files>::open(storage, key()).unwrap();

        tree.load_all().unwrap();
        let tree_index = &tree.index().tree;

        assert!(tree_index.file(&file_path).unwrap().is_none());
        assert_eq!(
            tree_index.file(&new_file_path).unwrap().unwrap().as_ref(),
            &entry
        );
    }

    #[test]
    fn test_iterate_all_files() {
        let tree = Tree::default();
        let file1 = "test/path/to/file.rs".to_string();
        let file2 = "test/path/file2.rs".to_string();

        _ = tree.insert_file(
            &file1,
            Entry {
                name: "file.rs".into(),
                size: 1234,
                ..Default::default()
            },
        );
        _ = tree.insert_file(
            &file1,
            Entry {
                name: "file.rs".into(),
                size: 4321,
                ..Default::default()
            },
        );
        _ = tree.insert_file(
            &file2,
            Entry {
                name: "file2.rs".into(),
                ..Default::default()
            },
        );

        let files = HashSet::new();
        _ = files.insert(file1);
        _ = files.insert(file2);

        for (k, v) in tree.iter_files() {
            files.remove(&k).unwrap();
            match v.name.as_ref() {
                "file.rs" => assert_eq!(v.size, 4321),
                "file2.rs" => (),
                _ => panic!("invalid file"),
            }
        }

        assert_eq!(files.len(), 0);
    }

    #[test]
    fn test_update_file() {
        let key = || {
            UsernamePassword::with_credentials("bare_index_map".to_string(), "password".to_string())
                .unwrap()
        };
        let storage = crate::backends::test::InMemoryBackend::shared();
        let file_path = "test/path/file.rs".to_string();
        let entry = Entry {
            name: String::from("file.rs"),
            ..Default::default()
        };
        let new_entry = Entry {
            name: String::from("new_file.rs"),
            ..Default::default()
        };

        {
            let mut tree = Infinitree::<Files>::empty(storage.clone(), key()).unwrap();

            {
                let tree_index = &tree.index().tree;

                // can't update what doesn't exist
                assert!(tree_index
                    .update_file(&file_path, new_entry.clone())
                    .is_err());

                tree_index.insert_file(&file_path, entry).unwrap();
                let entry_name = &tree_index.file(&file_path).unwrap().unwrap().name;

                assert_eq!(entry_name, "file.rs");
            }

            tree.commit(None).unwrap();
            tree.index().tree.clear();
            tree.load_all().unwrap();

            {
                let tree_index = &tree.index().tree;

                let entry_name = &tree_index.file(&file_path).unwrap().unwrap().name;
                assert_eq!(entry_name, "file.rs");

                let _ = tree_index.update_file(&file_path, new_entry);
                let entry_name = &tree_index.file(&file_path).unwrap().unwrap().name;
                assert_eq!(entry_name, "new_file.rs");
            }

            tree.commit(None).unwrap();
        }

        let tree = Infinitree::<Files>::open(storage, key()).unwrap();

        tree.load_all().unwrap();
        let tree_index = &tree.index().tree;
        let entry_name = &tree_index.file(&file_path).unwrap().unwrap().name;

        assert_eq!(entry_name, "new_file.rs");
    }

    #[test]
    fn test_index_can_be_restored() {
        let key = || {
            UsernamePassword::with_credentials("bare_index_map".to_string(), "password".to_string())
                .unwrap()
        };
        let storage = crate::backends::test::InMemoryBackend::shared();

        let file1 = "file1.rs".to_string();
        let file2 = "file2.rs".to_string();
        let file3 = "file3.rs".to_string();

        {
            let mut tree = Infinitree::<Files>::empty(storage.clone(), key()).unwrap();

            {
                let index = &tree.index().tree;
                index.insert_file(&file1, Entry::default()).unwrap();
                index.insert_file(&file2, Entry::default()).unwrap();
            }

            assert_eq!(tree.index().tree.0.len(), 3);
            tree.commit(None).unwrap();

            // clear restores the root node, so total length will be 1
            tree.index().tree.clear();
            assert_eq!(tree.index().tree.0.len(), 1);

            // reload the existing files
            tree.load_all().unwrap();

            {
                let index = &tree.index().tree;
                index
                    .insert_file(
                        &file1,
                        Entry {
                            name: "file1.rs".into(),
                            ..Default::default()
                        },
                    )
                    .unwrap();

                assert_eq!(
                    &tree.index().tree.file(&file1).unwrap().unwrap().name,
                    "file1.rs"
                );
                index.insert_file(&file3, Entry::default()).unwrap();
            }

            tree.commit(None).unwrap();
        }

        let tree = Infinitree::<Files>::open(storage, key()).unwrap();
        tree.load_all().unwrap();

        assert_eq!(
            &tree.index().tree.file(&file1).unwrap().unwrap().name,
            "file1.rs"
        );

        assert_eq!(tree.index().tree.0.len(), 4);
    }

    #[test]
    fn test_iter_all_files_2() {
        let key = || {
            UsernamePassword::with_credentials("bare_index_map".to_string(), "password".to_string())
                .unwrap()
        };
        let storage = crate::backends::test::InMemoryBackend::shared();
        let file1 = "test/path/to/file.rs".to_string();
        let file2 = "test/path/file.rs".to_string();
        let file3 = "test/file.rs".to_string();

        {
            let mut tree = Infinitree::<Files>::empty(storage.clone(), key()).unwrap();

            {
                tree.index()
                    .tree
                    .insert_file(&file1, Entry::default())
                    .unwrap();
                tree.index()
                    .tree
                    .insert_file(&file2, Entry::default())
                    .unwrap();
                assert_eq!(tree.index().tree.iter_files().count(), 2);
            }

            tree.commit(None).unwrap();
            tree.index().tree.clear();
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
    fn test_get_dir() {
        let tree = Tree::default();
        let file = "test/path/to/file.rs".to_string();
        tree.insert_file(&file, Entry::default()).unwrap();

        let node = tree.node_by_path("test/path/to").unwrap().unwrap();
        assert!(node.is_dir());
    }

    #[test]
    fn test_path_to_folder() {
        let tree = Tree::default();
        let file = "home/travel/pic.png".to_string();
        tree.insert_file(&file, Entry::default()).unwrap();
        let root = tree.root();

        let (root_ref, root_node, test_name) = tree.path_to_parent("home").unwrap();

        assert_eq!(Digest::default(), root_ref);
        assert_eq!(root.is_dir(), root_node.is_dir());
        assert_eq!("home", test_name);

        if let Node::Directory { entries } = root.as_ref().clone() {
            let (test_ref, test_node, to_name) = tree.path_to_parent("home/travel").unwrap();

            let t_ref = entries.read("home", |_, v| *v).unwrap();
            let t_node = tree.0.get(&t_ref).unwrap();

            assert_eq!(t_ref, test_ref);
            assert_eq!(t_node.is_dir(), test_node.is_dir());
            assert_eq!("travel", to_name);
        }
    }

    #[test]
    fn test_remove_dir() {
        let tree = Tree::default();
        let file = "home/travel/pic.png".to_string();
        let file2 = "home/travel/dogs/dog.png".to_string();
        tree.insert_file(&file, Entry::default()).unwrap();
        tree.insert_file(&file2, Entry::default()).unwrap();

        assert!(tree.remove("home/travel").is_ok());

        assert!(tree.node_by_path("home/travel").unwrap().is_none());
    }
}
