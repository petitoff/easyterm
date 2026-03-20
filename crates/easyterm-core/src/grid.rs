use crate::ansi::Style;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    pub text: String,
    pub style: Style,
    pub wide_continuation: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            text: String::new(),
            style: Style::default(),
            wide_continuation: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grid {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
}

impl Grid {
    pub fn new(width: usize, height: usize) -> Self {
        let width = width.max(1);
        let height = height.max(1);

        Self {
            width,
            height,
            cells: vec![Cell::default(); width * height],
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn get(&self, row: usize, col: usize) -> Option<&Cell> {
        if row >= self.height || col >= self.width {
            return None;
        }

        self.cells.get(row * self.width + col)
    }

    pub fn get_mut(&mut self, row: usize, col: usize) -> Option<&mut Cell> {
        if row >= self.height || col >= self.width {
            return None;
        }

        self.cells.get_mut(row * self.width + col)
    }

    pub fn row(&self, row: usize) -> Option<&[Cell]> {
        if row >= self.height {
            return None;
        }

        let start = row * self.width;
        let end = start + self.width;
        Some(&self.cells[start..end])
    }

    pub fn clear(&mut self) {
        self.cells.fill(Cell::default());
    }

    pub fn clear_row_from(&mut self, row: usize, start_col: usize) {
        let start_col = start_col.min(self.width.saturating_sub(1));
        for col in start_col..self.width {
            if let Some(cell) = self.get_mut(row, col) {
                *cell = Cell::default();
            }
        }
    }

    pub fn clear_row_to(&mut self, row: usize, end_col: usize) {
        let end_col = end_col.min(self.width.saturating_sub(1));
        for col in 0..=end_col {
            if let Some(cell) = self.get_mut(row, col) {
                *cell = Cell::default();
            }
        }
    }

    pub fn clear_row(&mut self, row: usize) {
        for col in 0..self.width {
            if let Some(cell) = self.get_mut(row, col) {
                *cell = Cell::default();
            }
        }
    }

    pub fn copy_row(&mut self, src_row: usize, dst_row: usize) {
        if src_row >= self.height || dst_row >= self.height {
            return;
        }

        for col in 0..self.width {
            let src = self.get(src_row, col).cloned().unwrap_or_default();
            if let Some(cell) = self.get_mut(dst_row, col) {
                *cell = src;
            }
        }
    }

    pub fn resize(&mut self, new_width: usize, new_height: usize) {
        let new_width = new_width.max(1);
        let new_height = new_height.max(1);
        let mut new_cells = vec![Cell::default(); new_width * new_height];

        let copy_height = self.height.min(new_height);
        let copy_width = self.width.min(new_width);

        for row in 0..copy_height {
            for col in 0..copy_width {
                let old_idx = row * self.width + col;
                let new_idx = row * new_width + col;
                new_cells[new_idx] = self.cells[old_idx].clone();
            }
        }

        self.width = new_width;
        self.height = new_height;
        self.cells = new_cells;
    }

    pub fn row_text(&self, row: usize) -> String {
        self.row(row).map(Self::cells_text).unwrap_or_default()
    }

    pub fn snapshot(&self) -> Vec<String> {
        (0..self.height).map(|row| self.row_text(row)).collect()
    }

    pub fn cells_text(cells: &[Cell]) -> String {
        let mut out = String::new();
        for cell in cells {
            if !cell.wide_continuation {
                out.push_str(&cell.text);
            }
        }
        out.trim_end().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{Cell, Grid};

    #[test]
    fn resize_preserves_existing_content() {
        let mut grid = Grid::new(3, 2);
        grid.get_mut(0, 0).unwrap().text = "a".into();
        grid.get_mut(1, 2).unwrap().text = "z".into();

        grid.resize(5, 3);

        assert_eq!(grid.get(0, 0).unwrap().text, "a");
        assert_eq!(grid.get(1, 2).unwrap().text, "z");
        assert_eq!(grid.get(2, 4).unwrap(), &Cell::default());
    }

    #[test]
    fn snapshot_ignores_wide_continuations() {
        let mut grid = Grid::new(3, 1);
        grid.get_mut(0, 0).unwrap().text = "界".into();
        grid.get_mut(0, 1).unwrap().wide_continuation = true;
        grid.get_mut(0, 2).unwrap().text = "x".into();

        assert_eq!(grid.snapshot(), vec!["界x"]);
    }
}
