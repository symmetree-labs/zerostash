use std::sync::{atomic::AtomicUsize, Arc};

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
pub struct Tree(InnerTree);

// auto-derive will not work for resolving constraints properly
impl Clone for Tree {
    fn clone(&self) -> Self {
        Tree(self.0.clone())
    }
}

impl Default for Tree {
    fn default() -> Self {
        let tree = Tree(VersionedMap::default());
        _ = tree.0.insert(Digest::default(), Node::directory());
        tree
    }
}

#[derive(Serialize, Deserialize, Clone)]
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

    pub fn update_file<'a>(&self, path: &'a str, file: Entry) -> Result<'a, ()> {
        let file_ref = self.get_ref(path)?.ok_or(FsError::NoSuchFileOrDirectory)?;
        self.0
            .update_with(file_ref, |_| Node::file(file))
            .ok_or(FsError::InvalidFilesystem)?;
        Ok(())
    }

    /// Recursively remove a subtree the `path`
    pub fn remove<'a>(&self, path: &'a str) -> Result<'a, ()> {
        let (parent_ref, parent, filename) = self.path_to_parent(path)?;
        let stack = scc::Stack::default();

        let Node::Directory { ref entries } = parent.as_ref() else {
	    unreachable!();
	};

        let file_node_ref = entries
            .read(filename, |_, v| *v)
            .ok_or(FsError::NoSuchFileOrDirectory)?;

        stack.push((parent_ref, file_node_ref, filename.to_string()));

        while let Some(next) = stack.pop().as_deref() {
            let (parent_ref, noderef, filename) = (next.0, next.1, next.2.to_string());
            let file_node = self.0.get(&noderef);

            match file_node.as_deref() {
                // if it's a file, we simply remove it along with the
                // file node
                Some(Node::File { .. }) => {
                    self.remove_file(&parent_ref, &filename);
                    self.0.remove(file_node_ref);
                }

                // if it's a directory, things are bit more complex
                Some(Node::Directory { ref entries }) => {
                    if entries.is_empty() {
                        // if it's empty, just delete it
                        //
                        self.remove_file(&parent_ref, &filename);
                        self.0.remove(noderef);
                    } else {
                        // if not empty, queue it for visit again
                        //
                        stack.push((parent_ref, noderef, filename.to_string()));

                        // then push all the children as well
                        //
                        entries.scan(|file, childref| {
                            stack.push((noderef, *childref, file.to_string()));
                        });
                    }
                }

                // technically this should not happen, maybe error
                // reporting could be better
                None => return Err(FsError::InvalidFilesystem),
            }
        }

        Ok(())
    }

    /// Return a file
    pub fn get_file<'a>(&self, path: &'a str) -> Result<'a, Option<Arc<Entry>>> {
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
        let Some(noderef) = self.get_ref(path)? else {
	    return Ok(None);
	};

        let Some(node) = self.0.get(&noderef) else {
	    return Err(FsError::InvalidFilesystem);
	};

        Ok(Some(node))
    }

    /// Move the file from the old path to the new path in the tree
    pub fn move_file<'a>(&self, old_path: &'a str, new_path: &'a str) -> Result<'a, ()> {
        // remove from the old place
        let (current_ref, _, filename) = self.path_to_parent(old_path)?;
        let noderef = {
            let mut noderef = None;
            self.0.update_with(current_ref, |v| {
                let current = v.as_ref().clone();
                let Node::Directory { ref entries } = current else {
		    unreachable!()
		};
                noderef = entries.remove(filename);
                current
            });

            let Some((_, noderef)) = noderef else {
		return Err(FsError::NoSuchFileOrDirectory);
            };

            noderef
        };

        // add to the new place, overwriting an existing file there
        let (new_ref, _, new_filename) = self.path_to_parent(new_path)?;
        self.0.update_with(new_ref, |v| {
            let new = v.as_ref().clone();
            let Node::Directory { ref entries } = new else {
		unreachable!()
	    };
            _ = entries.insert(new_filename.into(), noderef);
            new
        });

        Ok(())
    }

    fn root(&self) -> Arc<Node> {
        self.0.get(&Digest::default()).expect("uninitialized tree")
    }

    fn remove_file(&self, parent_ref: &Digest, filename: &str) {
        self.0.update_with(*parent_ref, |v| {
            let new = v.as_ref().clone();
            let Node::Directory { ref entries } = new else {
		unreachable!()
	    };
            entries.remove(filename);
            new
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
    /// # Errors
    ///
    /// Returns a list of valid path components, followed by the
    /// erroring component.
    fn path_to_parent<'a>(&self, path: &'a str) -> Result<'a, (Digest, Arc<Node>, &'a str)> {
        let mut parts = path.split('/').filter(|s| !s.is_empty()).peekable();

        let mut current = Some(self.root());
        let mut current_ref = Some(Digest::default());
        let mut consumed = vec![];

        while let Some(_next) = parts.peek() {
            let Some(part) = parts.next() else {
		unreachable!();
	    };
            consumed.push(part);

            let Some(Node::Directory { ref entries }) = current.as_deref() else {
		return Err(FsError::InvalidPath(consumed))
	    };

            current_ref = entries.read(part, |_, v| *v);
            current = current_ref.and_then(|nodeid| self.0.get(&nodeid));
        }

        let parent = current.ok_or(FsError::InvalidPath(consumed))?;
        Ok((
            current_ref.expect("must be Some"),
            parent,
            parts.next().unwrap(),
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
        let mut parts = path.split('/').filter(|s| !s.is_empty()).peekable();

        let mut parent = Digest::default();
        let mut current_ref = Some(Digest::default());
        let mut current = Some(self.root());
        let mut consumed = vec![];

        while let Some(_next) = parts.peek() {
            let Some(part) = parts.next() else {
		unreachable!();
	    };
            consumed.push(part);

            match current.as_deref() {
                Some(Node::Directory { ref entries }) => {
                    parent = current_ref.expect("current is Some");
                    current_ref = entries.read(part, |_, v| *v);
                    current = current_ref.and_then(|noderef| self.0.get(&noderef));
                }
                Some(_) => {
                    return Err(FsError::InvalidPath(consumed));
                }
                None => {
                    let (new_current_ref, new_current) = self.add_empty_dir(&parent, part);
                    current_ref = Some(new_current_ref);
                    current = Some(new_current);
                }
            }
        }

        Ok((
            current_ref.expect("must be Some"),
            current.expect("must be Some"),
            parts.last().unwrap(),
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

    pub fn iter_files(&self) -> TreeIterator {
        let stack = vec![(String::new(), Digest::default())];
        TreeIterator { stack, inner: self }
    }
}

pub struct TreeIterator<'a> {
    stack: Vec<(String, Digest)>,
    inner: &'a Tree,
}

impl<'a> Iterator for TreeIterator<'a> {
    type Item = (String, Arc<Entry>);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((prefix, node)) = self.stack.pop() {
            match self.inner.0.get(&node).unwrap().as_ref() {
                Node::File { refs: _, entry } => return Some((prefix, entry.clone())),
                Node::Directory { entries } => {
                    entries.for_each(|name, digest| {
                        let path = if prefix.is_empty() {
                            name.to_string()
                        } else {
                            format!("{prefix}/{name}")
                        };
                        self.stack.push((path, *digest));
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
