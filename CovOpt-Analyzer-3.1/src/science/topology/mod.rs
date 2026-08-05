use std::collections::HashSet;

/// Abstract traits for generic Graph Topology representation
pub trait GraphNode: Copy + Eq + std::hash::Hash {}
pub trait GraphEdge: Copy + Eq {}

/// Abstract Graph Interface
pub trait Graph<N: GraphNode, E: GraphEdge> {
    fn get_neighbors(&self, node: N, edge_type: E) -> Vec<N>;
    fn get_nodes(&self) -> Vec<N>;
}

/// Generic engine for Graph algorithms
pub struct TopologyEngine;

impl TopologyEngine {
    /// Generic Depth First Search to find a path between `start` and `target` using `edge_type`
    pub fn find_path<N, E, G>(graph: &G, start: N, edge_type: E, target: N) -> Option<Vec<N>>
    where
        N: GraphNode,
        E: GraphEdge,
        G: Graph<N, E>,
    {
        let mut path = Vec::new();
        let mut visited = HashSet::new();
        if Self::dfs(graph, start, edge_type, target, &mut visited, &mut path) {
            path.reverse();
            Some(path)
        } else {
            None
        }
    }

    fn dfs<N, E, G>(
        graph: &G,
        current: N,
        edge_type: E,
        target: N,
        visited: &mut HashSet<N>,
        path: &mut Vec<N>,
    ) -> bool
    where
        N: GraphNode,
        E: GraphEdge,
        G: Graph<N, E>,
    {
        if current == target {
            path.push(current);
            return true;
        }
        if !visited.insert(current) {
            return false;
        }

        for next_node in graph.get_neighbors(current, edge_type) {
            if Self::dfs(graph, next_node, edge_type, target, visited, path) {
                path.push(current);
                return true;
            }
        }
        false
    }

    /// Detects if adding an edge from `start` to `target` would create a cycle.
    pub fn would_create_cycle<N, E, G>(graph: &G, start: N, edge_type: E, target: N) -> bool
    where
        N: GraphNode,
        E: GraphEdge,
        G: Graph<N, E>,
    {
        Self::find_path(graph, target, edge_type, start).is_some()
    }
}
