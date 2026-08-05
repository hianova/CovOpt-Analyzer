use petgraph::Undirected;
use petgraph::graph::{Graph, NodeIndex};
use std::collections::HashMap;

/// Represents a Tensor Network simulating quantum entanglement.
/// We use an undirected graph where nodes are qubits/tensors and edges are entanglement bonds.
pub struct TensorNetwork {
    pub graph: Graph<usize, f32, Undirected>,
    node_map: HashMap<usize, NodeIndex>,
}

impl Default for TensorNetwork {
    fn default() -> Self {
        Self::new()
    }
}

impl TensorNetwork {
    pub fn new() -> Self {
        Self {
            graph: Graph::new_undirected(),
            node_map: HashMap::new(),
        }
    }

    /// Adds a particle (qubit) to the network.
    pub fn add_particle(&mut self, id: usize) {
        let idx = self.graph.add_node(id);
        self.node_map.insert(id, idx);
    }

    /// Adds an entanglement bond between two particles. The weight represents entanglement strength.
    pub fn entangle(&mut self, id1: usize, id2: usize, strength: f32) {
        if let (Some(&n1), Some(&n2)) = (self.node_map.get(&id1), self.node_map.get(&id2)) {
            self.graph.add_edge(n1, n2, strength);
        }
    }

    /// Implements Area Law Pruning.
    /// In a fully entangled system, complexity is O(N) volume.
    /// By pruning distant or weak entanglement, we reduce it to O(sqrt(N)) boundary area,
    /// leaving a localized tree-like structure.
    pub fn prune_distant_entanglement(&mut self, threshold: f32) {
        self.graph
            .retain_edges(|graph, edge_idx| graph[edge_idx] >= threshold);
    }

    /// Returns the total entanglement (sum of all bond strengths).
    pub fn total_entanglement(&self) -> f32 {
        self.graph.edge_weights().sum()
    }

    /// Returns the number of active entanglement bonds.
    pub fn bond_count(&self) -> usize {
        self.graph.edge_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_area_law_pruning() {
        let mut tn = TensorNetwork::new();

        // Add 4 particles
        for i in 0..4 {
            tn.add_particle(i);
        }

        // Strong local entanglement
        tn.entangle(0, 1, 0.9);
        tn.entangle(1, 2, 0.85);
        tn.entangle(2, 3, 0.95);

        // Weak distant entanglement (volume expansion)
        tn.entangle(0, 3, 0.1);
        tn.entangle(1, 3, 0.2);

        assert_eq!(tn.bond_count(), 5);

        // Prune edges below threshold (simulate area law bounds)
        tn.prune_distant_entanglement(0.5);

        // Should only have the 3 strong local edges left (localized tree)
        assert_eq!(tn.bond_count(), 3);
    }
}
