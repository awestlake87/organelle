use std::fmt;
use std::io::{self, Write};

#[derive(Debug, Copy, Clone)]
pub enum Compass {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

#[derive(Debug, Copy, Clone)]
pub enum SelectorKind {
    Graph,
    Node,
    Edge,
}

#[derive(Debug, Clone)]
pub struct Selector {
    kind: SelectorKind,
    attrs: Vec<Attribute>,
}

impl Selector {
    pub fn graph() -> Self {
        Self {
            kind: SelectorKind::Graph,
            attrs: vec![],
        }
    }

    pub fn node() -> Self {
        Self {
            kind: SelectorKind::Node,
            attrs: vec![],
        }
    }

    pub fn edge() -> Self {
        Self {
            kind: SelectorKind::Edge,
            attrs: vec![],
        }
    }

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

#[derive(Debug, Copy, Clone)]
pub enum EdgeOp {
    Directed,
}

#[derive(Debug, Clone)]
pub enum Id {
    Ident(String),
    Quoted(String),
}

impl Id {
    pub fn ident<T: Into<String>>(id: T) -> Self {
        Id::Ident(id.into())
    }

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

#[derive(Debug, Clone)]
pub struct NodeId {
    id: Id,
    port: Option<Id>,
    compass: Option<Compass>,
}

impl NodeId {
    pub fn new(id: Id) -> Self {
        Self {
            id: id,
            port: None,
            compass: None,
        }
    }

    pub fn port(self, port: Id) -> Self {
        Self {
            port: Some(port),
            ..self
        }
    }

    pub fn compass(self, compass: Compass) -> Self {
        Self {
            compass: Some(compass),
            ..self
        }
    }

    pub fn connect<T: Into<Edge>>(self, op: EdgeOp, rhs: T) -> Edge {
        Edge::from(self).connect(op, rhs)
    }

    fn write(&self, writer: &mut Write) -> io::Result<()> {
        write!(writer, "{}", self.id)
    }
}

#[derive(Debug, Clone)]
pub enum Edge {
    Node(NodeId),
    SubGraph(SubGraph),
    Edge {
        lhs: Box<Edge>,
        op: EdgeOp,
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
    pub fn node(node: NodeId) -> Self {
        Edge::Node(node)
    }

    pub fn subgraph(subgraph: SubGraph) -> Self {
        Edge::SubGraph(subgraph)
    }

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

#[derive(Debug, Clone)]
pub struct SubGraph {
    strict: bool,

    id: Option<Id>,

    statements: Vec<Statement>,
}

impl SubGraph {
    pub fn new() -> Self {
        Self {
            strict: false,
            id: None,
            statements: vec![],
        }
    }

    pub fn strict(self) -> Self {
        Self {
            strict: true,
            ..self
        }
    }

    pub fn id(self, id: Id) -> Self {
        Self {
            id: Some(id),
            ..self
        }
    }

    pub fn add<T: Into<Statement>>(mut self, statement: T) -> Self {
        self.statements.push(statement.into());

        self
    }

    fn write(&self, writer: &mut Write, indents: u32) -> io::Result<()> {
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

#[derive(Debug, Clone)]
pub enum Dot {
    DiGraph(SubGraph),
}

impl Dot {
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

#[derive(Debug, Clone)]
pub enum Statement {
    Node(Node),
    Edge(Edge),
    Selector(Selector),
    Attribute(Attribute),
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
