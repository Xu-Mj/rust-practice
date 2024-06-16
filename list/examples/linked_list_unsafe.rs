use std::fmt::{Display, Formatter, write};
use std::ptr::NonNull;

fn main() {
    let mut list = LinkedList::new();
    list.push(1);
    list.push(1);
    list.push(1);
    println!("{}", list)
}

#[derive(Debug)]
pub struct Node<T> {
    next: Option<NonNull<Node<T>>>,
    value: T,
}

impl<T> Node<T> {
    pub fn new(value: T) -> Self {
        Self {
            next: None,
            value,
        }
    }
}

impl<T> Display for Node<T>
    where T: Display {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.next {
            None => { write!(f, "{}", self.value) }
            Some(next) => { write!(f, "{} {}", self.value, unsafe { next.as_ref() }) }
        }
    }
}

#[derive(Debug)]
pub struct LinkedList<T> {
    head: Option<NonNull<Node<T>>>,
    size: usize,
}

impl<T> Display for LinkedList<T>
    where T: Display {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.head {
            None => { Ok(()) }
            Some(node) => { write!(f, "{}", unsafe { node.as_ref() }) }
        }
    }
}

impl<T> LinkedList<T> {
    pub fn new() -> Self {
        Self {
            head: None,
            size: 0,
        }
    }

    pub fn push(&mut self, value: T) -> &mut Self {
        let node = Node::new(value);
        unsafe {
            let node = Box::new(node);
            let node = NonNull::from(Box::leak(node));
            match self.head {
                None => { self.head = Some(node) }
                Some(head) => {
                    (*node.as_ptr()).next = self.head.take();
                    self.head = Some(node);
                }
            }
        }
        self.size += 1;
        self
    }
}

