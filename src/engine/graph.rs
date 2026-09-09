//! Real-time execution of a compiled routing graph.

use crate::routing::{CompiledRouting, Edge, EdgeKind};

/// Maximum combined delay storage accepted by one graph revision.
pub const MAX_COMPENSATION_FRAMES: usize = 1_920_000;

#[derive(Clone, Copy, Debug)]
pub struct NodeInput<'a> {
    pub node: u16,
    pub samples: &'a [[f32; 2]],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphRenderError {
    BufferSize,
    InvalidNode,
    InputSize,
    CompensationCapacity,
}

struct NodeBuffers {
    main: Vec<[f32; 2]>,
    sidechain: Vec<[f32; 2]>,
    pre_fader: Vec<[f32; 2]>,
    output: Vec<[f32; 2]>,
}

impl NodeBuffers {
    fn new(frames: usize) -> Self {
        Self {
            main: vec![[0.0; 2]; frames],
            sidechain: vec![[0.0; 2]; frames],
            pre_fader: vec![[0.0; 2]; frames],
            output: vec![[0.0; 2]; frames],
        }
    }

    fn clear(&mut self, frames: usize) {
        self.main[..frames].fill([0.0; 2]);
        self.sidechain[..frames].fill([0.0; 2]);
        self.pre_fader[..frames].fill([0.0; 2]);
        self.output[..frames].fill([0.0; 2]);
    }
}

struct RouteState {
    edge: Edge,
    delay: Vec<[f32; 2]>,
    cursor: usize,
}

impl RouteState {
    fn mix(&mut self, source: &[[f32; 2]], destination: &mut [[f32; 2]]) {
        for (source, destination) in source.iter().zip(destination) {
            let sample = if self.delay.is_empty() {
                *source
            } else {
                let delayed = self.delay[self.cursor];
                self.delay[self.cursor] = *source;
                self.cursor += 1;
                if self.cursor == self.delay.len() {
                    self.cursor = 0;
                }
                delayed
            };
            destination[0] += sample[0] * self.edge.gain;
            destination[1] += sample[1] * self.edge.gain;
        }
    }
}

/// Preallocated executor. Construction belongs on the control thread.
pub struct GraphRenderer {
    order: Vec<u16>,
    routes: Vec<RouteState>,
    nodes: Vec<NodeBuffers>,
    max_frames: usize,
}

impl GraphRenderer {
    pub fn new(compiled: &CompiledRouting, max_frames: usize) -> Result<Self, GraphRenderError> {
        if max_frames == 0 || max_frames > super::MAX_FRAMES {
            return Err(GraphRenderError::BufferSize);
        }
        let mut routes = Vec::with_capacity(compiled.edges().len());
        let mut compensation_frames = 0_usize;
        for (index, edge) in compiled.edges().iter().copied().enumerate() {
            let delay = compiled
                .edge_delay(index)
                .ok_or(GraphRenderError::CompensationCapacity)?;
            let delay =
                usize::try_from(delay).map_err(|_| GraphRenderError::CompensationCapacity)?;
            compensation_frames = compensation_frames
                .checked_add(delay)
                .filter(|frames| *frames <= MAX_COMPENSATION_FRAMES)
                .ok_or(GraphRenderError::CompensationCapacity)?;
            let mut storage = Vec::new();
            storage
                .try_reserve_exact(delay)
                .map_err(|_| GraphRenderError::CompensationCapacity)?;
            storage.resize(delay, [0.0; 2]);
            routes.push(RouteState {
                edge,
                delay: storage,
                cursor: 0,
            });
        }
        Ok(Self {
            order: compiled.order().to_vec(),
            routes,
            nodes: (0..compiled.order().len())
                .map(|_| NodeBuffers::new(max_frames))
                .collect(),
            max_frames,
        })
    }

    /// Render one block. The callback writes signal before and after the
    /// node fader so pre-fader sends preserve their routing point.
    pub fn render(
        &mut self,
        inputs: &[NodeInput<'_>],
        output_node: u16,
        output: &mut [[f32; 2]],
        mut process: impl FnMut(u16, &[[f32; 2]], &[[f32; 2]], &mut [[f32; 2]], &mut [[f32; 2]]),
    ) -> Result<(), GraphRenderError> {
        let frames = output.len();
        if frames > self.max_frames {
            return Err(GraphRenderError::BufferSize);
        }
        if usize::from(output_node) >= self.nodes.len() {
            return Err(GraphRenderError::InvalidNode);
        }
        for input in inputs {
            if usize::from(input.node) >= self.nodes.len() {
                return Err(GraphRenderError::InvalidNode);
            }
            if input.samples.len() != frames {
                return Err(GraphRenderError::InputSize);
            }
        }

        for node in &mut self.nodes {
            node.clear(frames);
        }
        for input in inputs {
            let destination = &mut self.nodes[usize::from(input.node)].main[..frames];
            for (destination, source) in destination.iter_mut().zip(input.samples) {
                destination[0] += source[0];
                destination[1] += source[1];
            }
        }

        for order_index in 0..self.order.len() {
            let node = self.order[order_index];
            let buffers = &mut self.nodes[usize::from(node)];
            process(
                node,
                &buffers.main[..frames],
                &buffers.sidechain[..frames],
                &mut buffers.pre_fader[..frames],
                &mut buffers.output[..frames],
            );
            for route_index in 0..self.routes.len() {
                if self.routes[route_index].edge.source != node {
                    continue;
                }
                let edge = self.routes[route_index].edge;
                let (source, destination) = two_nodes_mut(
                    &mut self.nodes,
                    usize::from(edge.source),
                    usize::from(edge.destination),
                );
                let source = match edge.kind {
                    EdgeKind::SendPreFader => &source.pre_fader[..frames],
                    _ => &source.output[..frames],
                };
                let destination = match edge.kind {
                    EdgeKind::Sidechain => &mut destination.sidechain[..frames],
                    _ => &mut destination.main[..frames],
                };
                self.routes[route_index].mix(source, destination);
            }
        }
        output.copy_from_slice(&self.nodes[usize::from(output_node)].output[..frames]);
        Ok(())
    }
}

fn two_nodes_mut(
    nodes: &mut [NodeBuffers],
    first: usize,
    second: usize,
) -> (&mut NodeBuffers, &mut NodeBuffers) {
    debug_assert_ne!(first, second);
    if first < second {
        let (left, right) = nodes.split_at_mut(second);
        (&mut left[first], &mut right[0])
    } else {
        let (left, right) = nodes.split_at_mut(first);
        (&mut right[0], &mut left[second])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::{Edge, EdgeKind, RoutingGraph};

    fn passthrough(
        _: u16,
        main: &[[f32; 2]],
        _: &[[f32; 2]],
        pre: &mut [[f32; 2]],
        post: &mut [[f32; 2]],
    ) {
        pre.copy_from_slice(main);
        post.copy_from_slice(main);
    }

    #[test]
    fn compensation_delays_the_shorter_path_without_allocating_in_render() {
        let mut graph = RoutingGraph::new(3).unwrap();
        graph.set_node_latency(0, 2).unwrap();
        graph
            .add_edge(Edge {
                source: 0,
                destination: 2,
                kind: EdgeKind::Main,
                gain: 1.0,
            })
            .unwrap();
        graph
            .add_edge(Edge {
                source: 1,
                destination: 2,
                kind: EdgeKind::Main,
                gain: 1.0,
            })
            .unwrap();
        let mut renderer = GraphRenderer::new(&graph.compile().unwrap(), 8).unwrap();
        let impulse = [[1.0, 1.0], [0.0; 2], [0.0; 2], [0.0; 2]];
        let mut output = [[0.0; 2]; 4];
        renderer
            .render(
                &[NodeInput {
                    node: 1,
                    samples: &impulse,
                }],
                2,
                &mut output,
                passthrough,
            )
            .unwrap();
        assert_eq!(output, [[0.0; 2], [0.0; 2], [1.0, 1.0], [0.0; 2]]);
    }

    #[test]
    fn pre_fader_sends_and_sidechains_use_separate_signals() {
        let mut graph = RoutingGraph::new(2).unwrap();
        graph
            .add_edge(Edge {
                source: 0,
                destination: 1,
                kind: EdgeKind::SendPreFader,
                gain: 1.0,
            })
            .unwrap();
        let mut renderer = GraphRenderer::new(&graph.compile().unwrap(), 4).unwrap();
        let input = [[1.0, 1.0]; 2];
        let mut output = [[0.0; 2]; 2];
        renderer
            .render(
                &[NodeInput {
                    node: 0,
                    samples: &input,
                }],
                1,
                &mut output,
                |node, main, _, pre, post| {
                    pre.copy_from_slice(main);
                    for (post, main) in post.iter_mut().zip(main) {
                        *post = if node == 0 {
                            [main[0] * 0.25, main[1] * 0.25]
                        } else {
                            *main
                        };
                    }
                },
            )
            .unwrap();
        assert_eq!(output, input);
    }

    #[test]
    fn invalid_blocks_are_rejected_before_output_changes() {
        let graph = RoutingGraph::new(1).unwrap().compile().unwrap();
        let mut renderer = GraphRenderer::new(&graph, 4).unwrap();
        let mut output = [[7.0; 2]; 5];
        assert_eq!(
            renderer.render(&[], 0, &mut output, passthrough),
            Err(GraphRenderError::BufferSize)
        );
        assert_eq!(output, [[7.0; 2]; 5]);
    }

    #[test]
    fn excessive_compensation_is_rejected_before_allocation() {
        let mut graph = RoutingGraph::new(3).unwrap();
        graph.set_node_latency(0, u32::MAX).unwrap();
        for source in 0..2 {
            graph
                .add_edge(Edge {
                    source,
                    destination: 2,
                    kind: EdgeKind::Main,
                    gain: 1.0,
                })
                .unwrap();
        }
        assert!(matches!(
            GraphRenderer::new(&graph.compile().unwrap(), 4),
            Err(GraphRenderError::CompensationCapacity)
        ));
    }
}
