#[derive(Debug, Default, Clone)]
pub struct Scheduler {
    pub needs_layout: bool,
    pub needs_paint: bool,
}

impl Scheduler {
    pub fn mark_layout(&mut self) {
        self.needs_layout = true;
        self.needs_paint = true;
    }

    pub fn mark_paint(&mut self) {
        self.needs_paint = true;
    }

    pub fn clear(&mut self) {
        self.needs_layout = false;
        self.needs_paint = false;
    }
}
