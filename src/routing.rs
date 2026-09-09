//! Fixed-capacity audio routing graph compilation.
//!
//! Graph edits and compilation run on the control thread. The compiled result
//! stores a deterministic processing order and the delay required on each edge
//! to align every input at a destination.

pub const MAX_NODES: usize = crate::mixer::MAX_TRACKS + 1;
/// Explicit routes plus one implicit master route per track.
pub const MAX_EDGES: usize = 1_280;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeKind {
    #[default]
    Main,
    SendPreFader,
    SendPostFader,
    Sidechain,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edge {
    pub source: u16,
    pub destination: u16,
    pub kind: EdgeKind,
    pub gain: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutingError {
    NodeCapacity,
    EdgeCapacity,
    InvalidNode,
    InvalidGain,
    DuplicateEdge,
    Cycle,
    LatencyOverflow,
}

/// Editable fixed-capacity graph for tracks, returns, groups, and output buses.
pub struct RoutingGraph {
    node_count: usize,
    node_latency: [u32; MAX_NODES],
    edges: [Edge; MAX_EDGES],
    edge_count: usize,
}

impl RoutingGraph {
    pub fn new(node_count: usize) -> Result<Self, RoutingError> {
        if node_count > MAX_NODES {
            return Err(RoutingError::NodeCapacity);
        }
        Ok(Self {
            node_count,
            node_latency: [0; MAX_NODES],
            edges: [Edge::default(); MAX_EDGES],
            edge_count: 0,
        })
    }

    pub const fn node_count(&self) -> usize {
        self.node_count
    }

    pub const fn edge_count(&self) -> usize {
        self.edge_count
    }

    pub fn set_node_latency(&mut self, node: u16, frames: u32) -> Result<(), RoutingError> {
        let index = self.node_index(node)?;
        self.node_latency[index] = frames;
        Ok(())
    }

    pub fn node_latency(&self, node: u16) -> Option<u32> {
        self.node_latency
            .get(usize::from(node))
            .copied()
            .filter(|_| usize::from(node) < self.node_count)
    }

    pub fn add_edge(&mut self, edge: Edge) -> Result<usize, RoutingError> {
        self.node_index(edge.source)?;
        self.node_index(edge.destination)?;
        if !edge.gain.is_finite() || !(0.0..=4.0).contains(&edge.gain) {
            return Err(RoutingError::InvalidGain);
        }
        if self.edges[..self.edge_count].iter().any(|candidate| {
            candidate.source == edge.source
                && candidate.destination == edge.destination
                && candidate.kind == edge.kind
        }) {
            return Err(RoutingError::DuplicateEdge);
        }
        if self.edge_count == MAX_EDGES {
            return Err(RoutingError::EdgeCapacity);
        }
        let index = self.edge_count;
        self.edges[index] = edge;
        self.edge_count += 1;
        Ok(index)
    }

    pub fn edge(&self, index: usize) -> Option<Edge> {
        self.edges
            .get(index)
            .copied()
            .filter(|_| index < self.edge_count)
    }

    pub fn compile(&self) -> Result<CompiledRouting, RoutingError> {
        let mut indegree = [0_u16; MAX_NODES];
        for edge in &self.edges[..self.edge_count] {
            let destination = usize::from(edge.destination);
            indegree[destination] = indegree[destination]
                .checked_add(1)
                .ok_or(RoutingError::EdgeCapacity)?;
        }

        let mut emitted = [false; MAX_NODES];
        let mut order = [0_u16; MAX_NODES];
        for slot in order.iter_mut().take(self.node_count) {
            let Some(node) =
                (0..self.node_count).find(|index| !emitted[*index] && indegree[*index] == 0)
            else {
                return Err(RoutingError::Cycle);
            };
            emitted[node] = true;
            *slot = node as u16;
            for edge in &self.edges[..self.edge_count] {
                if usize::from(edge.source) == node {
                    let destination = usize::from(edge.destination);
                    indegree[destination] -= 1;
                }
            }
        }

        let mut arrival_latency = [0_u32; MAX_NODES];
        let mut output_latency = [0_u32; MAX_NODES];
        for node in &order[..self.node_count] {
            let node_index = usize::from(*node);
            let mut arrival = 0_u32;
            for edge in &self.edges[..self.edge_count] {
                if edge.destination == *node {
                    arrival = arrival.max(output_latency[usize::from(edge.source)]);
                }
            }
            arrival_latency[node_index] = arrival;
            output_latency[node_index] = arrival
                .checked_add(self.node_latency[node_index])
                .ok_or(RoutingError::LatencyOverflow)?;
        }

        let mut edge_delay = [0_u32; MAX_EDGES];
        for (index, edge) in self.edges[..self.edge_count].iter().enumerate() {
            edge_delay[index] = arrival_latency[usize::from(edge.destination)]
                - output_latency[usize::from(edge.source)];
        }

        Ok(CompiledRouting {
            node_count: self.node_count,
            edges: self.edges,
            edge_count: self.edge_count,
            order,
            edge_delay,
            arrival_latency,
            output_latency,
        })
    }

    fn node_index(&self, node: u16) -> Result<usize, RoutingError> {
        let index = usize::from(node);
        if index < self.node_count {
            Ok(index)
        } else {
            Err(RoutingError::InvalidNode)
        }
    }
}

/// Processing order and plugin delay compensation for one graph revision.
pub struct CompiledRouting {
    node_count: usize,
    edges: [Edge; MAX_EDGES],
    edge_count: usize,
    order: [u16; MAX_NODES],
    edge_delay: [u32; MAX_EDGES],
    arrival_latency: [u32; MAX_NODES],
    output_latency: [u32; MAX_NODES],
}

impl CompiledRouting {
    pub const fn node_count(&self) -> usize {
        self.node_count
    }

    pub fn order(&self) -> &[u16] {
        &self.order[..self.node_count]
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges[..self.edge_count]
    }

    pub fn edge_delay(&self, index: usize) -> Option<u32> {
        self.edge_delay
            .get(index)
            .copied()
            .filter(|_| index < self.edge_count)
    }

    pub fn arrival_latency(&self, node: u16) -> Option<u32> {
        self.arrival_latency
            .get(usize::from(node))
            .copied()
            .filter(|_| usize::from(node) < self.node_count)
    }

    pub fn output_latency(&self, node: u16) -> Option<u32> {
        self.output_latency
            .get(usize::from(node))
            .copied()
            .filter(|_| usize::from(node) < self.node_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(source: u16, destination: u16) -> Edge {
        Edge {
            source,
            destination,
            kind: EdgeKind::Main,
            gain: 1.0,
        }
    }

    #[test]
    fn independent_nodes_compile_in_index_order() {
        let graph = RoutingGraph::new(4).unwrap();
        let compiled = graph.compile().unwrap();
        assert_eq!(compiled.order(), &[0, 1, 2, 3]);
        assert_eq!(compiled.output_latency(3), Some(0));
    }

    #[test]
    fn processing_order_follows_every_route() {
        let mut graph = RoutingGraph::new(5).unwrap();
        graph.add_edge(edge(2, 3)).unwrap();
        graph.add_edge(edge(0, 2)).unwrap();
        graph.add_edge(edge(3, 4)).unwrap();
        graph.add_edge(edge(1, 3)).unwrap();
        let compiled = graph.compile().unwrap();
        assert_eq!(compiled.order(), &[0, 1, 2, 3, 4]);
    }

    #[test]
    fn inputs_are_delayed_to_the_longest_arrival() {
        let mut graph = RoutingGraph::new(4).unwrap();
        graph.set_node_latency(0, 128).unwrap();
        graph.set_node_latency(1, 32).unwrap();
        graph.set_node_latency(2, 64).unwrap();
        let slow = graph.add_edge(edge(0, 3)).unwrap();
        let chained = graph.add_edge(edge(1, 2)).unwrap();
        let medium = graph.add_edge(edge(2, 3)).unwrap();
        let compiled = graph.compile().unwrap();
        assert_eq!(compiled.edge_delay(slow), Some(0));
        assert_eq!(compiled.edge_delay(chained), Some(0));
        assert_eq!(compiled.edge_delay(medium), Some(32));
        assert_eq!(compiled.arrival_latency(3), Some(128));
        assert_eq!(compiled.output_latency(3), Some(128));
    }

    #[test]
    fn sends_and_sidechains_participate_in_compensation() {
        let mut graph = RoutingGraph::new(3).unwrap();
        graph.set_node_latency(0, 240).unwrap();
        let main = graph.add_edge(edge(0, 2)).unwrap();
        let sidechain = graph
            .add_edge(Edge {
                source: 1,
                destination: 2,
                kind: EdgeKind::Sidechain,
                gain: 1.0,
            })
            .unwrap();
        let compiled = graph.compile().unwrap();
        assert_eq!(compiled.edge_delay(main), Some(0));
        assert_eq!(compiled.edge_delay(sidechain), Some(240));
    }

    #[test]
    fn cycles_and_latency_overflow_are_rejected() {
        let mut cyclic = RoutingGraph::new(2).unwrap();
        cyclic.add_edge(edge(0, 1)).unwrap();
        cyclic.add_edge(edge(1, 0)).unwrap();
        assert!(matches!(cyclic.compile(), Err(RoutingError::Cycle)));

        let mut overflow = RoutingGraph::new(2).unwrap();
        overflow.set_node_latency(0, u32::MAX).unwrap();
        overflow.set_node_latency(1, 1).unwrap();
        overflow.add_edge(edge(0, 1)).unwrap();
        assert!(matches!(
            overflow.compile(),
            Err(RoutingError::LatencyOverflow)
        ));
    }

    #[test]
    fn invalid_and_duplicate_edges_do_not_change_the_graph() {
        let mut graph = RoutingGraph::new(2).unwrap();
        assert_eq!(graph.add_edge(edge(0, 2)), Err(RoutingError::InvalidNode));
        assert_eq!(
            graph.add_edge(Edge {
                gain: f32::NAN,
                ..edge(0, 1)
            }),
            Err(RoutingError::InvalidGain)
        );
        graph.add_edge(edge(0, 1)).unwrap();
        assert_eq!(graph.add_edge(edge(0, 1)), Err(RoutingError::DuplicateEdge));
        assert_eq!(graph.edge_count(), 1);
    }
}
