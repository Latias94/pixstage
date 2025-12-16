#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        Some(Self {
            x,
            y,
            width,
            height,
        })
    }

    pub fn from_point(x: u32, y: u32) -> Self {
        Self {
            x,
            y,
            width: 1,
            height: 1,
        }
    }

    pub fn right(self) -> u32 {
        self.x.saturating_add(self.width)
    }

    pub fn bottom(self) -> u32 {
        self.y.saturating_add(self.height)
    }

    pub fn union(self, other: Rect) -> Rect {
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = self.right().max(other.right());
        let y1 = self.bottom().max(other.bottom());
        Rect {
            x: x0,
            y: y0,
            width: x1.saturating_sub(x0).max(1),
            height: y1.saturating_sub(y0).max(1),
        }
    }

    pub fn clamp_to(self, width: u32, height: u32) -> Option<Rect> {
        if width == 0 || height == 0 {
            return None;
        }

        if self.x >= width || self.y >= height {
            return None;
        }

        let right = self.right().min(width);
        let bottom = self.bottom().min(height);
        let clamped_width = right.saturating_sub(self.x);
        let clamped_height = bottom.saturating_sub(self.y);
        Rect::new(self.x, self.y, clamped_width, clamped_height)
    }
}

// Intentionally minimal; dirty tracking is implemented in `DirtyTiles`.
