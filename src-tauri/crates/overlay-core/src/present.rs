use crate::geom::PxRect;

/// 主线程按顺序执行的提交；只允许合并位图，不能合并状态变更。
#[derive(Debug, PartialEq)]
pub enum MainOp<F> {
    Present(F),
    Hide,
    RaiseTopmost,
    SetBounds(PxRect),
    MoveTo { x: i32, y: i32 },
    SetInteractive(bool),
}

pub struct CommitQueue<F> {
    ops: Vec<MainOp<F>>,
}

impl<F> Default for CommitQueue<F> {
    fn default() -> Self {
        Self::new()
    }
}

impl<F> CommitQueue<F> {
    pub fn new() -> Self {
        Self { ops: Vec::new() }
    }

    /// 新帧占据自己的顺序位置，其前的旧帧被释放，其余 op 保留原顺序。
    pub fn push(&mut self, op: MainOp<F>) {
        if matches!(op, MainOp::Present(_)) {
            self.ops.retain(|op| !matches!(op, MainOp::Present(_)));
        }
        self.ops.push(op);
    }

    pub fn take(&mut self) -> Vec<MainOp<F>> {
        std::mem::take(&mut self.ops)
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_present_preserves_hide_before_it() {
        let mut queue = CommitQueue::new();
        queue.push(MainOp::Present(1));
        queue.push(MainOp::Hide);
        queue.push(MainOp::Present(2));
        assert_eq!(queue.take(), vec![MainOp::Hide, MainOp::Present(2)]);
        assert!(queue.is_empty());
    }

    #[test]
    fn hide_after_last_present_remains_after_it() {
        let mut queue = CommitQueue::new();
        queue.push(MainOp::Present(1));
        queue.push(MainOp::Hide);
        assert_eq!(queue.take(), vec![MainOp::Present(1), MainOp::Hide]);
    }

    #[test]
    fn all_state_changes_keep_their_order_across_replaced_frames() {
        let mut queue = CommitQueue::new();
        queue.push(MainOp::Present(1));
        queue.push(MainOp::SetInteractive(false));
        queue.push(MainOp::SetBounds(PxRect::new(1, 2, 3, 4)));
        queue.push(MainOp::Present(2));
        queue.push(MainOp::MoveTo { x: 7, y: 8 });
        queue.push(MainOp::RaiseTopmost);
        queue.push(MainOp::SetInteractive(true));
        queue.push(MainOp::Present(3));
        assert_eq!(
            queue.take(),
            vec![
                MainOp::SetInteractive(false),
                MainOp::SetBounds(PxRect::new(1, 2, 3, 4)),
                MainOp::MoveTo { x: 7, y: 8 },
                MainOp::RaiseTopmost,
                MainOp::SetInteractive(true),
                MainOp::Present(3),
            ]
        );
        assert!(queue.take().is_empty());
    }

    #[test]
    fn replaced_frames_are_released_immediately() {
        use std::rc::Rc;
        let frame = Rc::new(());
        let mut queue = CommitQueue::new();
        queue.push(MainOp::Present(frame.clone()));
        queue.push(MainOp::Present(Rc::new(())));
        assert_eq!(Rc::strong_count(&frame), 1);
    }
}
