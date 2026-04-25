use crate::op::{Op, OpArg, OpCode};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DfPointer {
    Value { arg: OpArg },
    ValueType { arg: OpArg },
    OperationSourceType { index: usize },
    OperationTargetType { index: usize },
    Stack,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum DfAction {
    #[default]
    Push,
    Pop,
    Store,
    BranchMaybe,
    BranchAlways,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct DfOperation {
    action: DfAction,
    source: Option<DfPointer>,
    target: Option<DfPointer>,
}

#[derive(Debug, Clone)]
pub struct DfNode {
    pub instruction: usize,
    pub op: Op,
    pub operations: Vec<DfOperation>,
}

impl DfNode {
    pub fn new(instruction: usize, op: Op) -> Self {
        Self {
            op,
            instruction,
            operations: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DfAnalyzer {
    nodes: Vec<DfNode>,
}

impl DfAnalyzer {
    pub fn new() -> Self {
        Self::default()
    }

    fn process(&self, instruction: usize, op: &Op) -> Result<DfNode, DfError> {
        let mut node = DfNode::new(instruction, *op);
        match op.code {
            OpCode::Push => {
                node.operations.push(DfOperation {
                    action: DfAction::Push,
                    source: Some(DfPointer::Value { arg: op.arg }),
                    ..Default::default()
                });
            }

            OpCode::Pop => {
                node.operations.push(DfOperation {
                    action: DfAction::Pop,
                    source: Some(DfPointer::Stack),
                    ..Default::default()
                });
            }

            OpCode::Add | OpCode::Subtract | OpCode::Multiply | OpCode::Divide => {
                node.operations.push(DfOperation {
                    action: DfAction::Pop,
                    source: Some(DfPointer::Stack),
                    ..Default::default()
                });

                node.operations.push(DfOperation {
                    action: DfAction::Pop,
                    source: Some(DfPointer::Stack),
                    ..Default::default()
                });

                node.operations.push(DfOperation {
                    action: DfAction::Push,
                    source: None,
                    target: Some(DfPointer::OperationSourceType { index: 0 }),
                });
            }

            OpCode::JumpIfEqual => {}
            OpCode::Exit => {}
            OpCode::JumpIfZero => {}
            OpCode::Call => {}
            OpCode::Return => {}
            OpCode::Jump => {}
            OpCode::Remainder => {}
            OpCode::CountLeadingZeros => {}
            OpCode::CountLeadingOnes => {}
            OpCode::CountTrailingZeros => {}
            OpCode::CountTrailingOnes => {}
        }
        Ok(node)
    }

    pub fn prepare(&mut self, ops: &[Op]) -> Result<(), DfError> {
        for (instruction, op) in ops.iter().enumerate() {
            let node = self.process(instruction, op)?;
            self.nodes.push(node);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DfError {}
