use std::fmt;
use std::io::{self, Write};

/// where to aim the endpoint of an edge
#[derive(Debug, Copy, Clone)]
pub enum Compass {
    /// point to the top of a node
    North,
    /// point to the top-right of a node
    NorthEast,
    /// point to the right of a node
    East,
    /// point to the bottom-right of a node
    SouthEast,
    /// point to the bottom of a node
    South,
    /// point to the bottom-left of a node
    SouthWest,
    /// point to the left of a node
    West,
    /// point to the top-left of a node
    NorthWest,
}

/// apply attributes to nodes that match this selector
#[derive(Debug, Copy, Clone)]
pub enum SelectorKind {
    /// apply them to the subgraph itself
    Graph,
    /// apply them to all nodes within the subgraph
    Node,
    /// apply them to all edges within the subgraph
    Edge,
}

/// create a selector statement
#[derive(Debug, Clone)]
pub struct Selector {
    kind: SelectorKind,
    attrs: Vec<Attribute>,
}

impl Selector {
    /// select the graph
    pub fn graph() -> Self {
        Self {
            kind: SelectorKind::Graph,
            attrs: vec![],
        }
    }

    /// select all nodes
    pub fn node() -> Self {
        Self {
            kind: SelectorKind::Node,
            attrs: vec![],
        }
    }

    /// select all edges
    pub fn edge() -> Self {
        Self {
            kind: SelectorKind::Edge,
            attrs: vec![],
        }
    }

    /// add attributes to the selector
    pub fn add<T: Into<Attribute>>(mut self, attr: T) -> Self {
        self.attrs.push(attr.into());

        self
    }

    fn write(&self, writer: &mut Write) -> io::Result<()> {
        match self.kind {
            SelectorKind::Edge => write!(writer, "edge[")?,
            SelectorKind::Graph => write!(writer, "graph[")?,
            SelectorKind::Node => write!(writer, "node[")?,
        }

        for attr in &self.attrs {
            attr.write(writer)?;
            write!(writer, ",")?;
        }

        write!(writer, "]")?;

        Ok(())
    }
}

/// type of edge between two nodes
#[derive(Debug, Copy, Clone)]
pub enum EdgeOp {
    Directed,
}

/// an identifier in the DOT language
#[derive(Debug, Clone)]
pub enum Id {
    Ident(String),
    Quoted(String),
}

impl Id {
    /// create an unquoted alphanumeric identifier
    pub fn ident<T: Into<String>>(id: T) -> Self {
        Id::Ident(id.into())
    }

    /// create a quoted string that can contain any characters
    pub fn quoted<T>(id: T) -> Self
    where
        String: From<T>,
    {
        let s = String::from(id);

        Id::Quoted(s.replace("\"", "\\\""))
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &Id::Ident(ref id) => write!(f, "{}", id),
            &Id::Quoted(ref id) => write!(f, "\"{}\"", id),
        }
    }
}

/// identify a node with a port and compass
#[derive(Debug, Clone)]
pub struct NodeId {
    id: Id,
    port: Option<Id>,
    compass: Option<Compass>,
}

impl NodeId {
    /// identify a node
    pub fn new(id: Id) -> Self {
        Self {
            id: id,
            port: None,
            compass: None,
        }
    }

    /// add a port to the node id
    pub fn port(self, port: Id) -> Self {
        Self {
            port: Some(port),
            ..self
        }
    }

    /// point the edge to a part of the node
    pub fn compass(self, compass: Compass) -> Self {
        Self {
            compass: Some(compass),
            ..self
        }
    }

    /// create an edge from this node to another
    pub fn connect<T: Into<Edge>>(self, op: EdgeOp, rhs: T) -> Edge {
        Edge::from(self).connect(op, rhs)
    }

    fn write(&self, writer: &mut Write) -> io::Result<()> {
        write!(writer, "{}", self.id)?;

        if let Some(ref port) = self.port {
            write!(writer, ":{}", port)?;
        }

        if let Some(compass) = self.compass {
            write!(
                writer,
                ":{}",
                match compass {
                    Compass::North => "n",
                    Compass::NorthEast => "ne",
                    Compass::East => "e",
                    Compass::SouthEast => "se",
                    Compass::South => "s",
                    Compass::SouthWest => "sw",
                    Compass::West => "w",
                    Compass::NorthWest => "nw",
                }
            )?;
        }

        Ok(())
    }
}

/// an edge operand
#[derive(Debug, Clone)]
pub enum Edge {
    /// edge operands can be nodes
    Node(NodeId),
    /// edge operands can be subgraphs
    SubGraph(SubGraph),
    /// edge operands can be recursive
    Edge {
        /// lhs of an edge
        lhs: Box<Edge>,
        /// type of edge
        op: EdgeOp,
        /// rhs of an edge
        rhs: Box<Edge>,
    },
}

impl From<NodeId> for Edge {
    fn from(id: NodeId) -> Self {
        Edge::Node(id)
    }
}

impl From<SubGraph> for Edge {
    fn from(subgraph: SubGraph) -> Self {
        Edge::SubGraph(subgraph)
    }
}

impl Edge {
    /// add more operands to the edge
    pub fn connect<T: Into<Edge>>(self, op: EdgeOp, rhs: T) -> Self {
        Edge::Edge {
            lhs: Box::new(self),
            op: op,
            rhs: Box::new(rhs.into()),
        }
    }

    fn write(&self, writer: &mut Write) -> io::Result<()> {
        match self {
            &Edge::Node(ref node_id) => node_id.write(writer),
            &Edge::SubGraph(ref subgraph) => subgraph.write(writer, 0),
            &Edge::Edge {
                ref lhs,
                ref op,
                ref rhs,
            } => {
                lhs.write(writer)?;

                match op {
                    &EdgeOp::Directed => write!(writer, " -> ")?,
                }

                rhs.write(writer)
            },
        }
    }
}

/// a subgraph structure
#[derive(Debug, Clone)]
pub struct SubGraph {
    strict: bool,

    id: Option<Id>,

    statements: Vec<Statement>,
}

impl SubGraph {
    /// create a new subgraph
    pub fn new() -> Self {
        Self {
            strict: false,
            id: None,
            statements: vec![],
        }
    }

    /// make the subgraph strict
    pub fn strict(self) -> Self {
        Self {
            strict: true,
            ..self
        }
    }

    /// set the name of the subgraph
    pub fn id(self, id: Id) -> Self {
        Self {
            id: Some(id),
            ..self
        }
    }

    /// add statements to the body of the subgraph
    pub fn add<T: Into<Statement>>(mut self, statement: T) -> Self {
        self.statements.push(statement.into());

        self
    }

    fn write(&self, writer: &mut Write, indents: u32) -> io::Result<()> {
        write_indents(writer, indents)?;

        if self.strict {
            write!(writer, "strict ")?;
        }

        if let &Some(ref id) = &self.id {
            write!(writer, "subgraph {} ", id)?;
        }

        write_indents(writer, indents)?;
        write!(writer, "{{\n")?;

        for stmt in &self.statements {
            stmt.write(writer, indents + 1)?;
        }

        write_indents(writer, indents)?;
        write!(writer, "}}\n")
    }
}

fn write_indents(writer: &mut Write, indents: u32) -> io::Result<()> {
    for _ in 0..indents {
        write!(writer, "    ")?;
    }

    Ok(())
}

/// the root AST node
#[derive(Debug, Clone)]
pub enum Dot {
    /// create a directed graph
    DiGraph(SubGraph),
}

impl Dot {
    /// render the AST to the writer
    pub fn render(&self, writer: &mut Write) -> io::Result<()> {
        self.write(writer, 0)
    }

    fn write(&self, writer: &mut Write, indents: u32) -> io::Result<()> {
        match self {
            &Dot::DiGraph(ref subgraph) => {
                write_indents(writer, indents)?;

                if subgraph.strict {
                    write!(writer, "strict ")?;
                }

                write!(writer, "digraph ")?;

                if let Some(ref id) = subgraph.id {
                    write!(writer, "{} ", id)?;
                }

                write!(writer, "{{\n")?;

                for stmt in &subgraph.statements {
                    stmt.write(writer, indents + 1)?;
                }

                write_indents(writer, indents)?;
                write!(writer, "}}\n")?;

                Ok(())
            },
        }
    }
}

/// a DOT statement
#[derive(Debug, Clone)]
pub enum Statement {
    /// declare a standalone node
    Node(Node),
    /// declare an edge or set of edges
    Edge(Edge),
    /// apply attributes to a set of nodes/edges
    Selector(Selector),
    /// an attribute statement
    Attribute(Attribute),
    /// a subgraph statement
    SubGraph(SubGraph),
}

impl From<Node> for Statement {
    fn from(node: Node) -> Self {
        Statement::Node(node)
    }
}

impl From<Edge> for Statement {
    fn from(edge: Edge) -> Self {
        Statement::Edge(edge)
    }
}

impl From<Selector> for Statement {
    fn from(selector: Selector) -> Self {
        Statement::Selector(selector)
    }
}

impl From<Attribute> for Statement {
    fn from(attr: Attribute) -> Self {
        Statement::Attribute(attr)
    }
}

impl From<SubGraph> for Statement {
    fn from(subgraph: SubGraph) -> Self {
        Statement::SubGraph(subgraph)
    }
}

impl Statement {
    fn write(&self, writer: &mut Write, indents: u32) -> io::Result<()> {
        match self {
            &Statement::Node(ref node) => {
                write_indents(writer, indents)?;

                node.write(writer)?;

                write!(writer, ";\n")
            },
            &Statement::Edge(ref edge) => {
                write_indents(writer, indents)?;
                edge.write(writer)?;
                write!(writer, ";\n")
            },
            &Statement::Selector(ref selector) => {
                write_indents(writer, indents)?;

                selector.write(writer)?;

                write!(writer, ";\n")
            },
            &Statement::Attribute(ref attr) => {
                write_indents(writer, indents)?;

                attr.write(writer)?;

                write!(writer, ";\n")
            },
            &Statement::SubGraph(ref subgraph) => {
                subgraph.write(writer, indents)
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct Node {
    id: Id,

    attrs: Vec<Attribute>,
}

impl Node {
    pub fn new(id: Id) -> Self {
        Self::with_attrs(id, vec![])
    }
    pub fn with_attrs(id: Id, attrs: Vec<Attribute>) -> Self {
        Node {
            id: id,

            attrs: attrs,
        }
    }

    pub fn add<T: Into<Attribute>>(mut self, attr: T) -> Self {
        self.attrs.push(attr.into());

        self
    }

    fn write(&self, writer: &mut Write) -> io::Result<()> {
        write!(writer, "{}", self.id)?;

        if self.attrs.len() >= 1 {
            write!(writer, " [")?;

            for attr in &self.attrs {
                attr.write(writer)?;
                write!(writer, ",")?;
            }

            write!(writer, "]")?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Attribute(Id, Id);

impl Attribute {
    pub fn new(attr: Id, value: Id) -> Self {
        Self { 0: attr, 1: value }
    }

    fn write(&self, writer: &mut Write) -> io::Result<()> {
        write!(writer, "{}={}", self.0, self.1)
    }
}

#[test]
fn test() {
    let dot = Dot::DiGraph(
        SubGraph::new()
            .id(Id::ident("testgraph"))
            .add(
                NodeId::new(Id::ident("A"))
                    .connect(EdgeOp::Directed, NodeId::new(Id::ident("B")))
                    .connect(EdgeOp::Directed, NodeId::new(Id::ident("C")))
                    .connect(EdgeOp::Directed, NodeId::new(Id::ident("D")))
                    .connect(EdgeOp::Directed, NodeId::new(Id::ident("E"))),
            )
            .add(
                NodeId::new(Id::ident("B"))
                    .connect(EdgeOp::Directed, NodeId::new(Id::ident("D"))),
            )
            .add(
                SubGraph::new()
                    .add(Selector::node().add(Attribute::new(
                        Id::ident("shape"),
                        Id::ident("box"),
                    )))
                    .add(
                        NodeId::new(Id::ident("1"))
                            .connect(
                                EdgeOp::Directed,
                                NodeId::new(Id::ident("2")),
                            )
                            .connect(
                                EdgeOp::Directed,
                                NodeId::new(Id::ident("3")),
                            ),
                    ),
            )
            .add(
                NodeId::new(Id::ident("E"))
                    .connect(EdgeOp::Directed, NodeId::new(Id::ident("1")))
                    .connect(EdgeOp::Directed, NodeId::new(Id::ident("C"))),
            )
            .add(
                SubGraph::new()
                    .add(Attribute::new(Id::ident("rank"), Id::ident("same")))
                    .add(Node::new(Id::ident("1")))
                    .add(Node::new(Id::ident("A"))),
            )
            .add(
                SubGraph::new()
                    .add(Attribute::new(Id::ident("rank"), Id::ident("same")))
                    .add(Node::new(Id::ident("3")))
                    .add(Node::new(Id::ident("D"))),
            ),
    );

    let stdout = io::stdout();

    dot.render(&mut stdout.lock()).unwrap();
}
