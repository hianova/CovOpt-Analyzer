use std::fmt::Debug;

/// Represents a region of the 2D grid in the MotifTree
#[derive(Clone, Debug, PartialEq)]
pub enum MotifNode {
    Solid0,
    Solid1,
    Mixed(Box<[MotifNode; 4]>), // top-left, top-right, bottom-left, bottom-right
}

#[derive(Clone, Debug, PartialEq)]
pub struct MotifTree {
    pub root: MotifNode,
    pub dimension: usize,
}

impl MotifTree {
    pub fn new(dimension: usize, root: MotifNode) -> Self {
        Self { root, dimension }
    }

    /// Packs a dense 1D slice (representing a 2D grid) into a sparse MotifTree
    pub fn pack(grid: &[bool], dimension: usize) -> Self {
        assert!(
            dimension.is_power_of_two(),
            "Dimension must be power of two"
        );
        assert_eq!(grid.len(), dimension * dimension);
        let root = Self::pack_recursive(grid, dimension, 0, 0, dimension);
        Self { root, dimension }
    }

    fn pack_recursive(
        grid: &[bool],
        grid_dim: usize,
        x: usize,
        y: usize,
        size: usize,
    ) -> MotifNode {
        if size == 1 {
            if grid[y * grid_dim + x] {
                return MotifNode::Solid1;
            } else {
                return MotifNode::Solid0;
            }
        }

        let half = size / 2;
        let tl = Self::pack_recursive(grid, grid_dim, x, y, half);
        let tr = Self::pack_recursive(grid, grid_dim, x + half, y, half);
        let bl = Self::pack_recursive(grid, grid_dim, x, y + half, half);
        let br = Self::pack_recursive(grid, grid_dim, x + half, y + half, half);

        // Optimize: if all 4 children are the same Solid, collapse them
        if tl == MotifNode::Solid0
            && tr == MotifNode::Solid0
            && bl == MotifNode::Solid0
            && br == MotifNode::Solid0
        {
            return MotifNode::Solid0;
        }
        if tl == MotifNode::Solid1
            && tr == MotifNode::Solid1
            && bl == MotifNode::Solid1
            && br == MotifNode::Solid1
        {
            return MotifNode::Solid1;
        }

        MotifNode::Mixed(Box::new([tl, tr, bl, br]))
    }

    /// Unpacks the MotifTree back into a dense 1D slice (2D grid)
    pub fn unpack(&self) -> Vec<bool> {
        let mut grid = vec![false; self.dimension * self.dimension];
        self.unpack_recursive(&self.root, &mut grid, self.dimension, 0, 0, self.dimension);
        grid
    }

    fn unpack_recursive(
        &self,
        node: &MotifNode,
        grid: &mut [bool],
        grid_dim: usize,
        x: usize,
        y: usize,
        size: usize,
    ) {
        match node {
            MotifNode::Solid0 => {
                // Do nothing, already false
            }
            MotifNode::Solid1 => {
                for dy in 0..size {
                    for dx in 0..size {
                        grid[(y + dy) * grid_dim + (x + dx)] = true;
                    }
                }
            }
            MotifNode::Mixed(children) => {
                let half = size / 2;
                self.unpack_recursive(&children[0], grid, grid_dim, x, y, half);
                self.unpack_recursive(&children[1], grid, grid_dim, x + half, y, half);
                self.unpack_recursive(&children[2], grid, grid_dim, x, y + half, half);
                self.unpack_recursive(&children[3], grid, grid_dim, x + half, y + half, half);
            }
        }
    }

    /// Performs a Lévy flight perturbation by randomly flipping a node at a random depth
    pub fn mutate_node(&mut self, mut seed: usize) {
        Self::mutate_node_recursive(&mut self.root, &mut seed, self.dimension);
    }

    fn mutate_node_recursive(node: &mut MotifNode, seed: &mut usize, size: usize) {
        *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let action = *seed % 100;

        // 10% chance to flip this entire block (Lévy flight large jump if size is big)
        if size == 1 || action < 10 {
            Self::flip_tree(node);
        } else if let MotifNode::Mixed(children) = node {
            *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let child_idx = *seed % 4;
            Self::mutate_node_recursive(&mut children[child_idx], seed, size / 2);
        } else {
            // It's solid but we didn't flip it. We can shatter it into Mixed.
            *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            if (*seed).is_multiple_of(2) {
                let current = node.clone();
                let mut children = Box::new([
                    current.clone(),
                    current.clone(),
                    current.clone(),
                    current.clone(),
                ]);
                *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let child_idx = *seed % 4;
                Self::mutate_node_recursive(&mut children[child_idx], seed, size / 2);

                // Check if it collapsed back
                if children[0] == children[1]
                    && children[1] == children[2]
                    && children[2] == children[3]
                {
                    *node = children[0].clone();
                } else {
                    *node = MotifNode::Mixed(children);
                }
            } else {
                Self::flip_tree(node);
            }
        }
    }

    fn flip_tree(node: &mut MotifNode) {
        match node {
            MotifNode::Solid0 => *node = MotifNode::Solid1,
            MotifNode::Solid1 => *node = MotifNode::Solid0,
            MotifNode::Mixed(children) => {
                Self::flip_tree(&mut children[0]);
                Self::flip_tree(&mut children[1]);
                Self::flip_tree(&mut children[2]);
                Self::flip_tree(&mut children[3]);
            }
        }
    }
}
