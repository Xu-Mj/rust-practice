fn main() {
    let list = List::new();
    let list = list.prepend(1);
    let list = list.prepend(1);
    println!("{:?}", list.len())
}

// 定义一个最简单的链表，不支持删改，仅支持头插法
#[derive(Debug)]
enum List {
    // 正常节点，节点值以及下一个节点指针
    Cons(i32, Box<List>),
    Nil,
}

impl List {
    // 创建空表
    pub fn new() -> Self {
        List::Nil
    }

    // 头插法
    pub fn prepend(self, element: i32) -> Self {
        Self::Cons(element, Box::new(self))
    }

    // 获取列表长度
    pub fn len(&self) -> usize {
        match *self {
            List::Cons(_, ref self_) => self_.len() + 1,
            List::Nil => 0
        }
    }
}
