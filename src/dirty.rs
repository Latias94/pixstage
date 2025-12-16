use crate::Rect;

#[derive(Debug, Clone)]
pub(crate) struct DirtyTiles {
    tile_size: u32,
    tiles_x: u32,
    tiles_y: u32,
    bits: Vec<u64>,
    dirty_tiles: u32,
    full: bool,
    width: u32,
    height: u32,
}

impl DirtyTiles {
    pub(crate) fn new(width: u32, height: u32, tile_size: u32) -> Self {
        let tile_size = tile_size.max(1);
        let (tiles_x, tiles_y) = tiles_dim(width, height, tile_size);
        let bit_len = (tiles_x as usize) * (tiles_y as usize);
        let bits = vec![0u64; (bit_len + 63) / 64];
        Self {
            tile_size,
            tiles_x,
            tiles_y,
            bits,
            dirty_tiles: 0,
            full: false,
            width,
            height,
        }
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        let (tiles_x, tiles_y) = tiles_dim(width, height, self.tile_size);
        self.tiles_x = tiles_x;
        self.tiles_y = tiles_y;
        let bit_len = (tiles_x as usize) * (tiles_y as usize);
        self.bits.clear();
        self.bits.resize((bit_len + 63) / 64, 0);
        self.dirty_tiles = 0;
        self.full = true;
    }

    pub(crate) fn mark_full(&mut self) {
        self.full = true;
    }

    pub(crate) fn mark_rect(&mut self, rect: Rect) {
        if self.full {
            return;
        }
        let Some(rect) = rect.clamp_to(self.width, self.height) else {
            return;
        };

        let tile = self.tile_size;
        let x0 = rect.x / tile;
        let y0 = rect.y / tile;
        let x1 = (rect.x + rect.width - 1) / tile;
        let y1 = (rect.y + rect.height - 1) / tile;

        for ty in y0..=y1 {
            for tx in x0..=x1 {
                self.set_tile(tx, ty);
            }
        }
    }

    pub(crate) fn mark_point(&mut self, x: u32, y: u32) {
        if self.full || x >= self.width || y >= self.height {
            return;
        }
        let tx = x / self.tile_size;
        let ty = y / self.tile_size;
        self.set_tile(tx, ty);
    }

    pub(crate) fn take_regions(&mut self, max_regions: usize) -> Vec<Rect> {
        if self.width == 0 || self.height == 0 {
            self.clear();
            return Vec::new();
        }

        if self.full {
            self.clear();
            return vec![Rect {
                x: 0,
                y: 0,
                width: self.width,
                height: self.height,
            }];
        }

        if self.dirty_tiles == 0 {
            return Vec::new();
        }

        let mut out: Vec<Rect> = Vec::new();
        let mut prev: Vec<Rect> = Vec::new();

        for ty in 0..self.tiles_y {
            let y = ty * self.tile_size;
            let mut runs: Vec<(u32, u32)> = Vec::new();

            let mut tx = 0;
            while tx < self.tiles_x {
                if !self.is_tile_set(tx, ty) {
                    tx += 1;
                    continue;
                }

                let start = tx;
                tx += 1;
                while tx < self.tiles_x && self.is_tile_set(tx, ty) {
                    tx += 1;
                }
                let end = tx;

                let x = start * self.tile_size;
                let width = (end - start) * self.tile_size;
                runs.push((x, width));
            }

            let mut next_prev: Vec<Rect> = Vec::with_capacity(runs.len());
            let mut prev_idx = 0usize;
            prev.sort_by_key(|rect| rect.x);

            for (x, width) in runs {
                while prev_idx < prev.len() && prev[prev_idx].x < x {
                    out.push(prev[prev_idx]);
                    prev_idx += 1;
                }

                if prev_idx < prev.len()
                    && prev[prev_idx].x == x
                    && prev[prev_idx].width == width
                    && prev[prev_idx].y + prev[prev_idx].height == y
                {
                    let mut rect = prev[prev_idx];
                    rect.height = rect.height.saturating_add(self.tile_size);
                    next_prev.push(rect);
                    prev_idx += 1;
                } else {
                    next_prev.push(Rect {
                        x,
                        y,
                        width,
                        height: self.tile_size,
                    });
                }
            }

            while prev_idx < prev.len() {
                out.push(prev[prev_idx]);
                prev_idx += 1;
            }

            prev = next_prev;
        }

        out.extend(prev);

        for rect in out.iter_mut() {
            if let Some(clamped) = rect.clamp_to(self.width, self.height) {
                *rect = clamped;
            } else {
                rect.width = 0;
                rect.height = 0;
            }
        }
        out.retain(|rect| rect.width > 0 && rect.height > 0);

        if out.len() > max_regions {
            let mut merged = out[0];
            for rect in out.iter().skip(1) {
                merged = merged.union(*rect);
            }
            out.clear();
            out.push(merged);
        }

        self.clear();
        out
    }

    fn clear(&mut self) {
        self.bits.fill(0);
        self.dirty_tiles = 0;
        self.full = false;
    }

    fn set_tile(&mut self, tx: u32, ty: u32) {
        if tx >= self.tiles_x || ty >= self.tiles_y {
            return;
        }
        let tile_index = (ty * self.tiles_x + tx) as usize;
        let word = tile_index / 64;
        let bit = tile_index % 64;
        let mask = 1u64 << bit;
        if (self.bits[word] & mask) == 0 {
            self.bits[word] |= mask;
            self.dirty_tiles = self.dirty_tiles.saturating_add(1);
        }
    }

    fn is_tile_set(&self, tx: u32, ty: u32) -> bool {
        let tile_index = (ty * self.tiles_x + tx) as usize;
        let word = tile_index / 64;
        let bit = tile_index % 64;
        (self.bits[word] & (1u64 << bit)) != 0
    }
}

fn tiles_dim(width: u32, height: u32, tile_size: u32) -> (u32, u32) {
    let tiles_x = (width + tile_size - 1) / tile_size;
    let tiles_y = (height + tile_size - 1) / tile_size;
    (tiles_x.max(1), tiles_y.max(1))
}
