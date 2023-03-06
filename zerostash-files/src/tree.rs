use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::FileType;

#[derive(Clone, Serialize, Deserialize, Default, PartialEq, Debug)]
pub struct File {
    pub name: String,
    pub file_type: FileType,
}

impl File {
    pub fn new(name: String, file_type: FileType) -> Self {
        Self { name, file_type }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum Node {
    File(File),
    Directory(Arc<Mutex<HashMap<String, Node>>>),
}

impl Default for Node {
    fn default() -> Self {
        Self::Directory(Arc::new(Mutex::new(HashMap::default())))
    }
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct Tree(Arc<Mutex<HashMap<String, Node>>>);

impl Tree {
    pub fn insert_directory(&mut self, path: &str, dir: Option<Node>) {
        let mut current = self.0.clone();
        let parts = path
            .split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        let parts_len = parts.len() - 1;

        let child = current
            .lock()
            .unwrap()
            .entry("".to_string())
            .or_insert_with(|| Node::Directory(Arc::new(Mutex::new(HashMap::new()))))
            .clone();

        current = match child {
            Node::Directory(dir) => dir,
            _ => panic!("Path is not valid"),
        };

        for part in parts.iter().take(parts_len) {
            let child = current
                .lock()
                .unwrap()
                .entry(part.to_string())
                .or_insert_with(|| Node::Directory(Arc::new(Mutex::new(HashMap::new()))))
                .clone();

            current = match child {
                Node::Directory(dir) => dir,
                _ => panic!("Path is not valid"),
            };
        }

        let filename = parts.last().expect("Path is not valid");
        if let Some(dir) = dir {
            current.lock().unwrap().insert(filename.to_string(), dir);
        } else {
            let _ = current
                .lock()
                .unwrap()
                .entry(filename.to_string())
                .or_insert_with(|| Node::Directory(Arc::new(Mutex::new(HashMap::new()))))
                .clone();
        }
    }

    pub fn insert_file(&mut self, path: &str, file: File) {
        let parts = path
            .split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        let parts_len = parts.len() - 1;
        let mut current = self.0.clone();

        let child = current
            .lock()
            .unwrap()
            .entry("".to_string())
            .or_insert_with(|| Node::Directory(Arc::new(Mutex::new(HashMap::new()))))
            .clone();

        current = match child {
            Node::Directory(dir) => dir,
            _ => panic!("Path is not valid"),
        };

        for part in parts.iter().take(parts_len) {
            let child = current
                .lock()
                .unwrap()
                .entry(part.to_string())
                .or_insert_with(|| Node::Directory(Arc::new(Mutex::new(HashMap::new()))))
                .clone();

            current = match child {
                Node::Directory(dir) => dir,
                _ => panic!("Path is not valid"),
            };
        }

        let filename = parts.last().expect("Path is not valid");
        let file_node = Node::File(file);
        current
            .lock()
            .unwrap()
            .insert(filename.to_string(), file_node);
    }

    pub fn remove(&mut self, path: &str) -> Option<Node> {
        let parts = path
            .split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        let parts_len = parts.len() - 1;
        let mut current = self.0.clone();

        let child = match current.lock().unwrap().get("") {
            Some(node) => node.clone(),
            None => return None,
        };

        current = match child {
            Node::Directory(dir) => dir,
            _ => return None,
        };

        for part in parts.iter().take(parts_len) {
            let child = match current.lock().unwrap().get(&part.to_string()) {
                Some(node) => node.clone(),
                None => return None,
            };

            current = match child {
                Node::Directory(dir) => dir,
                _ => return None,
            };
        }

        let directory_name = parts.last().expect("Path is not valid");
        let mut current = current.lock().unwrap();
        current.remove(&directory_name.to_string())
    }

    pub fn get(&self, path: &str) -> Option<Node> {
        let parts = path
            .split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        let mut current = self.0.clone();

        let child = current.lock().unwrap().get("").cloned();

        current = match child {
            Some(Node::Directory(dir)) => dir,
            _ => return None,
        };

        for part in parts {
            let child = current.lock().unwrap().get(part).cloned();

            current = match child {
                Some(Node::Directory(dir)) => dir,
                Some(node) => return Some(node),
                None => return None,
            };
        }

        Some(Node::Directory(current))
    }

    pub fn move_node(&mut self, old_path: &str, new_path: &str) {
        let node = self.get(old_path).unwrap();
        self.remove(old_path);
        match node {
            Node::File(file) => {
                self.insert_file(new_path, file);
            }
            Node::Directory(dir) => {
                self.insert_directory(new_path, Some(Node::Directory(dir)));
            }
        }
    }

    pub fn pretty_print(&self) {
        pretty_print_helper(&self.0.lock().unwrap().clone(), 0);
    }
}

pub fn pretty_print_helper(node: &HashMap<String, Node>, indent: usize) {
    for (name, child) in node {
        match child {
            Node::Directory(dir) => {
                println!("{:indent$}|- {name}/", "", indent = indent * 2, name = name);
                pretty_print_helper(&dir.lock().unwrap(), indent + 1);
            }
            Node::File(_) => {
                println!("{:indent$}|- {name}", "", indent = indent * 2, name = name,);
            }
        }
    }
}
