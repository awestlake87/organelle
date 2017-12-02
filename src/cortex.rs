
use std::collections::{ HashSet, HashMap };
use std::hash::Hash;
use std::mem;

use uuid::Uuid;

use super::{ Result, Lobe };

/// handle used to reference nodes within the cortex
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct NodeHdl {
    uuid: Uuid,
    index: usize
}

impl NodeHdl {
    fn new() -> Self {
        Self { uuid: Uuid::new_v4(), index: 0 }
    }

    fn with_index(index: usize) -> Self {
        Self { uuid: Uuid::new_v4(), index: index }
    }
}

/// trait used to validate connection constraints
pub trait CortexConstraints<C> {
    /// get required input constraints
    fn get_required_inputs(&self) -> Vec<C>;
    /// get variadic input constraints
    fn get_variadic_inputs(&self) -> Vec<C>;
    /// get optional input constraints
    fn get_optional_inputs(&self) -> Vec<C>;

    /// get output constraints
    fn get_outputs(&self) -> Vec<C>;

    /// get required input feedback
    fn get_required_feedback(&self) -> Vec<C>;
    /// get variadic input feedback
    fn get_variadic_feedback(&self) -> Vec<C>;
    /// get optional input feedback
    fn get_optional_feedback(&self) -> Vec<C>;

    /// get feedback outputs
    fn get_feedback_outputs(&self) -> Vec<C>;
}

/// a trait applied to nodes to compose a cortex
pub trait CortexNode<C, D>: CortexConstraints<C> {
    /// called with the initial data (use this to initalize node)
    fn start(
        &mut self, this: NodeHdl, inputs: Vec<NodeHdl>, outputs: Vec<NodeHdl>
    )
        -> Result<()>
    ;

    /// called with periodic updates to data
    fn update(&mut self, data: D) -> Result<()>;
    /// called per connection to tailor output
    fn tailor_output(&mut self, output: NodeHdl) -> Result<D>;

    /// called in reverse with feedback updates
    fn feedback(&mut self, data: D) -> Result<()>;
    /// called per input to tailor feedback
    fn tailor_feedback(&mut self, input: NodeHdl) -> Result<D>;

    /// called with the final data (use this to reset node)
    fn stop(&mut self) -> Result<()>;
}

struct Node<C, D> {
    hdl:                    NodeHdl,
    node:                   Box<CortexNode<C, D>>,

    input_data:             D,
    feedback_data:          D,

    inputs:                 HashMap<NodeHdl, Connection<C>>,
    outputs:                HashMap<NodeHdl, Connection<C>>,
}

struct Connection<C> {
    constraints: Vec<C>
}

/// the cortex connects the inputs and outputs to a network of cortex nodes
pub struct Cortex<C, D> where
    C: Copy + Clone + Eq + PartialEq + Hash,
    D: Default + Clone + CortexDistributor<C, D>
{
    node_pool:              Vec<Node<C, D>>,

    this_hdl:               NodeHdl,
    output_data:            D,
    feedback_data:          D,

    input_node:            NodeHdl,
    output_node:            NodeHdl,
}

/// the cortex builder takes nodes and connections and generates the cortex
pub struct CortexBuilder<C, D> where
    C: Copy + Clone + Eq + PartialEq + Hash,
    D: Default + Clone + CortexDistributor<C, D>
{
    node_pool:              Vec<Option<Node<C, D>>>,
    input_node:            Option<NodeHdl>,
    output_node:            Option<NodeHdl>,
}

impl<C, D> CortexBuilder<C, D> where
    C: Copy + Clone + Eq + PartialEq + Hash,
    D: Default + Clone + CortexDistributor<C, D>
{
    /// construct a new cortex
    pub fn new() -> Self {
        Self {
            node_pool: vec![ ],

            input_node: None,
            output_node: None
        }
    }

    fn get_node_order(&self) -> Result<Vec<usize>> {
        if self.input_node.is_none() {
            bail!("no input node specified")
        }

        if self.output_node.is_none() {
            bail!("no output node specified")
        }

        let mut node_order = Vec::with_capacity(
            self.node_pool.len()
        );
        let mut nodes_ordered = HashSet::with_capacity(
            self.node_pool.len()
        );
        let mut has_circle = false;

        // continue as long as graph has no circle and there are still nodes
        // remaining to be ordered
        while !has_circle && nodes_ordered.len() < self.node_pool.len()
        {
            // graph has a circle if no remaining nodes can be ordered
            let mut num_ordered = 0;

            for i in 0..self.node_pool.len() {
                if nodes_ordered.contains(&i) {
                    continue
                }

                let mut has_unordered_deps = false;

                if let Some(ref node) = self.node_pool[i] {
                    for (input, _) in &node.inputs {
                        if !nodes_ordered.contains(&input.index) {
                            has_unordered_deps = true;
                            break
                        }
                    }
                }

                if !has_unordered_deps {
                    nodes_ordered.insert(i);
                    node_order.push(i);

                    num_ordered += 1;
                }
            }

            if num_ordered == 0 {
                has_circle = true;
            }
        }

        if has_circle {
            bail!("nodes have circular dependencies")
        }

        Ok(node_order)
    }

    /// add a node to the node pool
    pub fn add_node(&mut self, node: Box<CortexNode<C, D>>) -> NodeHdl {
        let hdl = NodeHdl::with_index(self.node_pool.len());

        self.node_pool.push(
            Some(
                Node {
                    hdl: hdl,
                    node: node,

                    input_data: D::default(),
                    feedback_data: D::default(),

                    inputs: HashMap::new(),
                    outputs: HashMap::new()
                }
            )
        );

        hdl
    }

    /// connect the output of l1 to l2 with constraints
    pub fn connect(&mut self, l1: NodeHdl, l2: NodeHdl, constraints: Vec<C>)
        -> Result<()>
    {
        for c in &constraints {
            let mut has_output = false;
            let mut has_input = false;

            if let Some(ref node) = self.node_pool[l1.index] {
                for output in node.node.get_outputs() {
                    if *c == output {
                        has_output = true;
                        break
                    }
                }
            }

            if let Some(ref node) = self.node_pool[l2.index] {
                for input in node.node.get_required_inputs() {
                    if *c == input {
                        has_input = true;
                        break
                    }
                }
            }

            if !has_input {
                if let Some(ref node) = self.node_pool[l2.index] {
                    for input in node.node.get_variadic_inputs() {
                        if *c == input {
                            has_input = true;
                            break
                        }
                    }
                }
            }

            if !has_input {
                if let Some(ref node) = self.node_pool[l2.index] {
                    for input in node.node.get_optional_inputs() {
                        if *c == input {
                            has_input = true;
                            break
                        }
                    }
                }
            }

            if !has_output || !has_input {
                bail!("nodes do not follow the constraints")
            }
        }

        if let Some(ref mut node) = self.node_pool[l1.index] {
            node.outputs.insert(
                l2,
                Connection {
                    constraints: constraints
                }
            );
        }
        if let Some(ref mut node) = self.node_pool[l2.index] {
            node.inputs.insert(
                l1,
                Connection {
                    constraints: vec![ ]
                }
            );
        }

        Ok(())
    }

    /// connect the feedback l1 into l2 with constraints
    pub fn feedback(&mut self, l1: NodeHdl, l2: NodeHdl, constraints: Vec<C>)
        -> Result<()>
    {
        for c in &constraints {
            let mut has_output = false;
            let mut has_input = false;

            if let Some(ref node) = self.node_pool[l1.index] {
                for output in node.node.get_feedback_outputs() {
                    if *c == output {
                        has_output = true;
                        break
                    }
                }
            }

            if let Some(ref node) = self.node_pool[l2.index] {
                for input in node.node.get_required_feedback() {
                    if *c == input {
                        has_input = true;
                        break
                    }
                }
            }

            if !has_input {
                if let Some(ref node) = self.node_pool[l2.index] {
                    for input in node.node.get_variadic_feedback() {
                        if *c == input {
                            has_input = true;
                            break
                        }
                    }
                }
            }

            if !has_input {
                if let Some(ref node) = self.node_pool[l2.index] {
                    for input in node.node.get_optional_feedback() {
                        if *c == input {
                            has_input = true;
                            break
                        }
                    }
                }
            }

            if !has_output || !has_input {
                bail!("nodes do not follow the constraints")
            }
        }

        if let Some(ref mut node) = self.node_pool[l1.index] {
            if let Some(ref mut input) = node.inputs.get_mut(&l2) {
                input.constraints = constraints;
            }
            else {
                bail!("nodes are not connected yet")
            }
        }

        Ok(())
    }

    /// set the input node
    pub fn set_input(&mut self, input: NodeHdl) {
        self.input_node = Some(input);
    }

    /// set the output node
    pub fn set_output(&mut self, output: NodeHdl) {
        self.output_node = Some(output);
    }

    /// map the nodes to the correct network topology and create the cortex
    pub fn build(mut self) -> Result<Cortex<C, D>> {
        let order = self.get_node_order()?;

        let remap = {
            let mut hdl_mapping = HashMap::new();

            let mut i: usize = 0;
            for o in &order {
                hdl_mapping.insert(*o, i);
                i += 1;
            }

            move |hdl: NodeHdl| NodeHdl {
                uuid: hdl.uuid, index: hdl_mapping[&hdl.index]
            }
        };

        let mut nodes = vec![ ];

        for n in order {
            let node = mem::replace(&mut self.node_pool[n], None).unwrap();

            let mut inputs = HashMap::new();
            let mut outputs = HashMap::new();

            for (i, constraints) in node.inputs.into_iter() {
                inputs.insert(remap(i), constraints);
            }

            for (o, constraints) in node.outputs.into_iter() {
                outputs.insert(remap(o), constraints);
            }

            nodes.push(
                Node {
                    hdl: remap(node.hdl),
                    node: node.node,

                    input_data: D::default(),
                    feedback_data: D::default(),

                    inputs: inputs,
                    outputs: outputs,
                }
            )
        }

        Ok(
            Cortex::<C, D> {
                node_pool: nodes,

                this_hdl: NodeHdl::new(),
                output_data: D::default(),
                feedback_data: D::default(),

                input_node: remap(self.input_node.unwrap()),
                output_node: remap(self.output_node.unwrap())
            }
        )
    }
}

impl<C, D> Cortex<C, D> where
    C: Copy + Clone + Eq + PartialEq + Hash,
    D: Default + Clone + CortexDistributor<C, D>
{
    fn distribute_data(&mut self, hdl: NodeHdl) -> Result<()> {
        if hdl == self.output_node {
            self.output_data = self.node_pool[hdl.index].node.tailor_output(
                hdl
            )?;
        }
        else {
            let mut outputs = mem::replace(
                &mut self.node_pool[hdl.index].outputs, HashMap::new()
            );

            for (o, connection) in &mut outputs {
                let mut input = mem::replace(
                    &mut self.node_pool[o.index].input_data, D::default()
                );
                let output = self.node_pool[hdl.index].node.tailor_output(*o)?;

                output.distribute(&connection.constraints, &mut input)?;

                self.node_pool[o.index].input_data = input;
            }

            self.node_pool[hdl.index].outputs = outputs;
        }

        Ok(())
    }

    fn distribute_feedback(&mut self, hdl: NodeHdl) -> Result<()> {
        if hdl == self.input_node {
            self.feedback_data = self.node_pool[hdl.index].node.tailor_feedback(
                hdl
            )?;
        }
        else {
            let mut inputs = mem::replace(
                &mut self.node_pool[hdl.index].inputs, HashMap::new()
            );

            for (i, connection) in &mut inputs {
                let mut input = mem::replace(
                    &mut self.node_pool[i.index].feedback_data, D::default()
                );
                let output = self.node_pool[hdl.index].node.tailor_feedback(
                    *i
                )?;

                output.distribute(&connection.constraints, &mut input)?;

                self.node_pool[i.index].feedback_data = input;
            }

            self.node_pool[hdl.index].inputs = inputs;
        }

        Ok(())
    }

    fn start_node(&mut self, hdl: NodeHdl) -> Result<()> {
        let inputs = self.node_pool[hdl.index].inputs.iter()
            .map(|(hdl, _)| *hdl)
            .collect()
        ;
        let outputs = self.node_pool[hdl.index].outputs.iter()
            .map(|(hdl, _)| *hdl)
            .collect()
        ;

        self.node_pool[hdl.index].node.start(hdl, inputs, outputs)
    }

    fn update_node(&mut self, hdl: NodeHdl, data: D) -> Result<()> {
        self.node_pool[hdl.index].node.update(data)?;

        self.distribute_data(hdl)?;

        Ok(())
    }

    fn feedback_node(&mut self, hdl: NodeHdl, data: D) -> Result<()> {
        self.node_pool[hdl.index].node.feedback(data)?;

        self.distribute_feedback(hdl)?;

        Ok(())
    }

    fn stop_node(&mut self, hdl: NodeHdl) -> Result<()> {
        self.node_pool[hdl.index].node.stop()?;

        Ok(())
    }

    /// start the top-level cortex
    pub fn start_cortex(&mut self) -> Result<()> {
        let this = self.this_hdl;

        <Self as CortexNode<C, D>>::start(self, this, vec![ ], vec![ ])
    }

    /// update the top-level cortex with the given data
    pub fn update_cortex(&mut self, data: D) -> Result<D> {
        <Self as CortexNode<C, D>>::update(self, data)?;

        Ok(mem::replace(&mut self.output_data, D::default()))
    }

    /// feed data back through the top-level cortex
    pub fn feedback_cortex(&mut self, data: D) -> Result<D> {
        <Self as CortexNode<C, D>>::feedback(self, data)?;

        Ok(mem::replace(&mut self.feedback_data, D::default()))
    }

    /// stop the top-level cortex
    pub fn stop_cortex(&mut self) -> Result<()> {
        <Self as CortexNode<C, D>>::stop(self)
    }
}

impl<C, D> CortexConstraints<C> for Cortex<C, D> where
    C: Copy + Clone + Eq + PartialEq + Hash,
    D: Default + Clone + CortexDistributor<C, D>
{
    fn get_required_inputs(&self) -> Vec<C> {
        self.node_pool[self.input_node.index].node.get_required_inputs()
    }
    fn get_variadic_inputs(&self) -> Vec<C> {
        self.node_pool[self.input_node.index].node.get_variadic_inputs()
    }
    fn get_optional_inputs(&self) -> Vec<C> {
        self.node_pool[self.input_node.index].node.get_optional_inputs()
    }

    fn get_outputs(&self) -> Vec<C> {
        self.node_pool[self.output_node.index].node.get_outputs()
    }

    fn get_required_feedback(&self) -> Vec<C> {
        self.node_pool[self.output_node.index].node.get_required_feedback()
    }
    fn get_variadic_feedback(&self) -> Vec<C> {
        self.node_pool[self.output_node.index].node.get_variadic_feedback()
    }
    fn get_optional_feedback(&self) -> Vec<C> {
        self.node_pool[self.output_node.index].node.get_optional_feedback()
    }

    fn get_feedback_outputs(&self) -> Vec<C> {
        self.node_pool[self.input_node.index].node.get_feedback_outputs()
    }
}

impl<C, D> CortexNode<C, D> for Cortex<C, D> where
    C: Copy + Clone + Eq + PartialEq + Hash,
    D: Default + Clone + CortexDistributor<C, D>
{
    fn start(&mut self, _: NodeHdl, _: Vec<NodeHdl>, _: Vec<NodeHdl>)
        -> Result<()>
    {
        for l in 0..self.node_pool.len() {
            let hdl = self.node_pool[l].hdl;
            self.start_node(hdl)?;
        }

        Ok(())
    }

    fn update(&mut self, data: D) -> Result<()> {
        self.node_pool[self.input_node.index].input_data = data;

        for l in 0..self.node_pool.len() {
            let hdl = self.node_pool[l].hdl;
            let data = mem::replace(
                &mut self.node_pool[l].input_data, D::default()
            );

            self.update_node(hdl, data)?;
        }

        Ok(())
    }
    fn tailor_output(&mut self, _: NodeHdl) -> Result<D> {
        Ok(self.output_data.clone())
    }

    fn feedback(&mut self, data: D) -> Result<()> {
        self.node_pool[self.output_node.index].feedback_data = data;

        for l in (0..self.node_pool.len()).rev() {
            let hdl = self.node_pool[l].hdl;
            let data = mem::replace(
                &mut self.node_pool[l].feedback_data, D::default()
            );

            self.feedback_node(hdl, data)?;
        }

        Ok(())
    }
    fn tailor_feedback(&mut self, _: NodeHdl) -> Result<D> {
        Ok(self.feedback_data.clone())
    }

    fn stop(&mut self) -> Result<()> {
        for l in 0..self.node_pool.len() {
            let hdl = self.node_pool[l].hdl;
            self.stop_node(hdl)?;
        }

        Ok(())
    }
}

impl<C, D> Lobe for Cortex<C, D> where
    C: Copy + Clone + Eq + PartialEq + Hash,
    D: Default + Clone + CortexDistributor<C, D>
{
    type Input = D;
    type Output = D;
    type FeedbackInput = D;
    type FeedbackOutput = D;

    fn start(
        &mut self, this: NodeHdl, inputs: Vec<NodeHdl>, outputs: Vec<NodeHdl>
    )
        -> Result<()>
    {
        <Self as CortexNode<C, D>>::start(self, this, inputs, outputs)
    }

    fn update(&mut self, data: D) -> Result<()> {
        <Self as CortexNode<C, D>>::update(self, data)
    }
    fn tailor_output(&mut self, output: NodeHdl) -> Result<D> {
        <Self as CortexNode<C, D>>::tailor_output(self, output)
    }

    fn feedback(&mut self, data: D) -> Result<()> {
        <Self as CortexNode<C, D>>::feedback(self, data)
    }
    fn tailor_feedback(&mut self, input: NodeHdl) -> Result<D> {
        <Self as CortexNode<C, D>>::tailor_feedback(self, input)
    }

    fn stop(&mut self) -> Result<()> {
        <Self as CortexNode<C, D>>::stop(self)
    }
}

/// enum describing the variadicity of the link between two nodes
#[derive(Debug, Clone)]
pub enum CortexLink<T> {
    /// link has one singular piece of input data
    One(T),
    /// link has many pieces of input data
    Many(Vec<T>),
    /// link no input data
    Empty
}

impl<T> CortexLink<T> {
    /// try extracting a singular piece of input data
    pub fn try_unwrap_one(self) -> Result<T> {
        match self {
            CortexLink::One(data) => Ok(data),
            CortexLink::Many(_) => bail!("link contains many inputs"),
            CortexLink::Empty => bail!("link contains no inputs")
        }
    }
    /// add an input to the link data
    pub fn add(&mut self, new_data: T) {
        *self = match mem::replace(self, CortexLink::Empty) {
            CortexLink::Empty => CortexLink::One(new_data),
            CortexLink::One(data) => CortexLink::Many(vec![ data, new_data ]),
            CortexLink::Many(mut data) => {
                data.push(new_data);
                CortexLink::Many(data)
            }
        }
    }
}

/// distributes data to dependent nodes in the cortex
pub trait CortexDistributor<C, D> {
    /// distribute self to the outgoing data using the constraints
    fn distribute(self, constraints: &[C], outgoing: &mut D) -> Result<()>;
}

/// create a cortex and the associated types given the constraints
///
/// constraints represent the different types of data that can be relayed
/// throughout the network of lobes.
///
/// each cortex is given a single input constraint that must be obeyed by
/// the input lobe and a single output constraint that must be obeyed by the
/// output lobe.
///
/// each lobe in the network of the cortex can have required inputs, optional
/// inputs, and variadic inputs as well as a set of outputs. these constraints
/// should eventually tell another object how these lobes can interact with
/// each other and how they can be arranged and configured throughout the
/// network.
///
/// lobes can be linked together with the connect function by specifying an
/// input lobe, an output lobe, and a set of constraints that the connection
/// must obey. the input lobe must have these constraints in its outputs and
/// the output lobe must have these constraints in either its required,
/// optional, or variadic input constraints
#[macro_export]
macro_rules! create_cortex {
    {
        module: $module:ident,
        constraints: {
            $($name:ident: $data:ty),*
        },
        input: $input:ident,
        output: $output:ident
    } => {
        #[allow(unused_imports, dead_code, unreachable_patterns)]
        mod $module {
            use std::collections::{ HashMap, HashSet };
            use std::mem;
            use std::rc::Rc;

            use sc2;

            use $crate::{
                Result,
                CortexNode,
                CortexLink,
                NodeHdl,
                CortexDistributor
            };
            use super::*;

            #[allow(missing_docs)]
            #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
            pub enum Constraint {
                $($name,)*
            }

            #[allow(non_snake_case, missing_docs)]
            #[derive(Debug, Clone)]
            pub struct Data {
                $(pub $name: CortexLink<$data>,)*
            }

            impl Default for Data {
                fn default() -> Self {
                    Self {
                        $($name: CortexLink::Empty,)*
                    }
                }
            }

            /// wrapper around the cortex
            ///
            /// used to work around the orphan rule
            pub struct Cortex(pub $crate::Cortex<Constraint, Data>);

            impl CortexDistributor<Constraint, Data> for Data {
                fn distribute(
                    mut self,
                    constraints: &[Constraint],
                    outgoing: &mut Data
                )
                    -> Result<()>
                {
                    for c in constraints.iter() {
                        match *c {
                            $(
                                Constraint::$name => {
                                    outgoing.$name.add(
                                        mem::replace(
                                            &mut self.$name,
                                            CortexLink::Empty
                                        ).try_unwrap_one()?
                                    );
                                }
                            )*
                        }
                    }

                    Ok(())
                }
            }
        }
    }
}

/// add constraints to the lobe to tell the cortex how to interpret its data
#[macro_export]
macro_rules! constrain_cortex {
    {
        cortex: $cortex:ty,

        inner_constraint: $inner_constraint:ident,
        inner_data: $inner_data:ident,

        outer_constraint: $outer_constraint:ident,
        outer_data: $outer_data:ident,

        mapping: {
            $($inner:ident => $outer:ident),*
        }
    } => {
        impl $crate::FromCortexData<$outer_data> for $inner_data {
            fn from_cortex_data(outer: $outer_data)
                -> $crate::Result<$inner_data>
            {
                Ok(
                    $inner_data {
                        $($inner: outer.$outer,)*
                        ..<$inner_data as Default>::default()
                    }
                )
            }
        }

        impl $crate::IntoCortexData<$outer_data> for $inner_data {
            fn into_cortex_data(self) -> $crate::Result<$outer_data> {
                Ok(
                    $outer_data {
                        $($outer: self.$inner,)*
                        ..<$outer_data as Default>::default()
                    }
                )
            }
        }

        impl $crate::CortexConstraints<$outer_constraint> for $cortex {
            fn get_required_inputs(&self) -> Vec<$outer_constraint> {
                <
                    $crate::Cortex<$inner_constraint, $inner_data> as
                    $crate::CortexConstraints<$inner_constraint>
                >::get_required_inputs(&self.0).iter().filter_map(
                    |input| match *input {
                        $(
                            $inner_constraint::$inner => Some(
                                $outer_constraint::$outer
                            ),
                        )*
                        _ => None
                    }
                ).collect()
            }
            fn get_variadic_inputs(&self) -> Vec<$outer_constraint> {
                <
                    $crate::Cortex<$inner_constraint, $inner_data> as
                    $crate::CortexConstraints<$inner_constraint>
                >::get_variadic_inputs(&self.0).iter().filter_map(
                    |input| match *input {
                        $(
                            $inner_constraint::$inner => Some(
                                $outer_constraint::$outer
                            ),
                        )*
                        _ => None
                    }
                ).collect()
            }
            fn get_optional_inputs(&self) -> Vec<$outer_constraint> {
                <
                    $crate::Cortex<$inner_constraint, $inner_data> as
                    $crate::CortexConstraints<$inner_constraint>
                >::get_optional_inputs(&self.0).iter().filter_map(
                    |input| match *input {
                        $(
                            $inner_constraint::$inner => Some(
                                $outer_constraint::$outer
                            ),
                        )*
                        _ => None
                    }
                ).collect()
            }

            fn get_outputs(&self) -> Vec<$outer_constraint> {
                <
                    $crate::Cortex<$inner_constraint, $inner_data> as
                    $crate::CortexConstraints<$inner_constraint>
                >::get_outputs(&self.0).iter().filter_map(
                    |output| match *output {
                        $(
                            $inner_constraint::$inner => Some(
                                $outer_constraint::$outer
                            ),
                        )*
                        _ => None
                    }
                ).collect()
            }

            fn get_required_feedback(&self) -> Vec<$outer_constraint> {
                <
                    $crate::Cortex<$inner_constraint, $inner_data> as
                    $crate::CortexConstraints<$inner_constraint>
                >::get_required_feedback(&self.0).iter().filter_map(
                    |input| match *input {
                        $(
                            $inner_constraint::$inner => Some(
                                $outer_constraint::$outer
                            ),
                        )*
                        _ => None
                    }
                ).collect()
            }
            fn get_variadic_feedback(&self) -> Vec<$outer_constraint> {
                <
                    $crate::Cortex<$inner_constraint, $inner_data> as
                    $crate::CortexConstraints<$inner_constraint>
                >::get_variadic_feedback(&self.0).iter().filter_map(
                    |input| match *input {
                        $(
                            $inner_constraint::$inner => Some(
                                $outer_constraint::$outer
                            ),
                        )*
                        _ => None
                    }
                ).collect()
            }
            fn get_optional_feedback(&self) -> Vec<$outer_constraint> {
                <
                    $crate::Cortex<$inner_constraint, $inner_data> as
                    $crate::CortexConstraints<$inner_constraint>
                >::get_optional_feedback(&self.0).iter().filter_map(
                    |input| match *input {
                        $(
                            $inner_constraint::$inner => Some(
                                $outer_constraint::$outer
                            ),
                        )*
                        _ => None
                    }
                ).collect()
            }

            fn get_feedback_outputs(&self) -> Vec<$outer_constraint> {
                <
                    $crate::Cortex<$inner_constraint, $inner_data> as
                    $crate::CortexConstraints<$inner_constraint>
                >::get_feedback_outputs(&self.0).iter().filter_map(
                    |output| match *output {
                        $(
                            $inner_constraint::$inner => Some(
                                $outer_constraint::$outer
                            ),
                        )*
                        _ => None
                    }
                ).collect()
            }
        }

        impl $crate::CortexNode<$outer_constraint, $outer_data> for $cortex {
            fn start(
                &mut self,
                this: $crate::NodeHdl,
                inputs: Vec<$crate::NodeHdl>,
                outputs: Vec<$crate::NodeHdl>
            )
                -> $crate::Result<()>
            {
                self.0.start(this, inputs, outputs)
            }

            fn update(&mut self, outer: $outer_data) -> $crate::Result<()>
            {
                self.0.update(
                    <
                        $inner_data as $crate::FromCortexData<$outer_data>
                    >::from_cortex_data(outer)?
                )
            }
            fn tailor_output(&mut self, output: $crate::NodeHdl)
                -> $crate::Result<$outer_data>
            {
                Ok(
                    <
                        $inner_data as $crate::IntoCortexData<$outer_data>
                    >::into_cortex_data(self.0.tailor_output(output)?)?
                )
            }

            fn feedback(&mut self, outer: $outer_data) -> $crate::Result<()>
            {
                self.0.feedback(
                    <
                        $inner_data as $crate::FromCortexData<$outer_data>
                    >::from_cortex_data(outer)?
                )
            }
            fn tailor_feedback(&mut self, input: $crate::NodeHdl)
                -> $crate::Result<$outer_data>
            {
                Ok(
                    <
                        $inner_data as $crate::IntoCortexData<$outer_data>
                    >::into_cortex_data(self.0.tailor_feedback(input)?)?
                )
            }

            fn stop(&mut self) -> $crate::Result<()> {
                self.0.stop()
            }
        }
    }
}
