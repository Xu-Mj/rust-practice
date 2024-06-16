fn main() {
    let mut list = LinkedList::new();
    list.push(1);
    list.push(1);
    list.push(1);
    list.append(2);
    list.push(12);
    println!("{:?}", list.pop());
    println!("{:?}", list.pop());
    println!("{:?}", list.pop_back());
    println!("{:?}", list)
}

/// 创建一个节点对象，包含一个指向下一个节点的指针以及该节点所包含的值
#[derive(Debug)]
pub struct Node<T> {
    next: Option<Box<Node<T>>>,
    value: T,
}

/// 创建一个链表, 包含一个头节点和一个长度字段
#[derive(Debug)]
pub struct LinkedList<T> {
    head: Option<Box<Node<T>>>,
    size: usize,
}

impl<T> Node<T> {
    pub fn new(value: T) -> Self {
        Self {
            next: None,
            value,
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

    /// 头插法
    pub fn push(&mut self, element: T) -> &mut Self {
        // 创建一个node
        let node = Box::new(Node {
            next: self.head.take(),
            value: element,
        });
        self.head = Some(node);
        self.size += 1;
        self
    }

    /// 请注意，这里对链表的遍历不能使用current.next.as_mut()因为这里会对`current.next`产生借用，
    /// 而while循环结束后会重新赋值current.next这里是对current.next的可变借用，
    /// 因此会产生借用冲突
    ///
    /// 使用ref mut 模式匹配所产生的current.next的可变借用的作用在while循环内部，因此不会产生借用冲突
    ///
    /// 尾插法
    pub fn append(&mut self, value: T) -> &mut Self {
        let tail = Box::new(Node::new(value));
        // 遍历到最后一个节点，
        match self.head.as_mut() {
            Some(mut current) => {
                while let Some(ref mut next) = current.next {
                    current = next;
                }
                current.next = Some(tail);
            }
            None => self.head = Some(tail),
        }
        self.size += 1;
        self
    }

    pub fn len(&self) -> usize {
        self.size
    }

    /// 弹出第一个节点数据
    pub fn pop(&mut self) -> Option<T> {
        self.head.take().map(|node| {
            self.size -= 1;
            self.head = node.next;
            node.value
        })
    }

    /// 弹出最后一个
    pub fn pop_back(&mut self) -> Option<T> {
        // 空链表
        if self.head.is_none() {
            return None;
        }

        // 只有一个节点
        if self.head.as_ref().unwrap().next.is_none() {
            self.size -= 1;
            return self.head.take().map(|n| n.value);
        }

        // 超过一个节点
        // 遍历到倒数第二个节点
        let mut current = self.head.as_mut().unwrap();
        while current.next.as_mut().unwrap().next.is_some() {
            current = current.next.as_mut().unwrap();
        }
        self.size -= 1;
        current.next.take().map(|n| n.value)
    }
}
