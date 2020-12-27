//! Parse simple const expressions and calculate them on the fly.
//!
//! The grammar of expressions is defined as such:
//!
//! ```text
//! ConstExpr := Or
//!
//! Literal := Number literal (decimal, hex, octal or binary)
//! Or      := And ('|' And)*
//! And     := Shift ('&' Shift)*
//! Shift   := Sum ('<<' Sum | '>>' Sum)?
//! Sum     := Mul ('+' Mul | '-' Mul)*
//! Mul     := Unary ('*' Unary | '/' Unary)*
//! Unary   := Expr | '-' Expr | '~' Expr
//! Expr    := Literal | '(' ConstExpr ')'
//! ```
//!
//! All the calculation is done with the [`Value`](type.Value.html) type, then converted down using the
//! `TryFrom` trait.

use std::convert::TryFrom;

use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{char, space0},
    combinator::{map, opt, value},
    multi::fold_many0,
    sequence::preceded,
    IResult,
};
use thiserror::Error;

use super::{
    literal::parse_number_literal,
    location::{AbsoluteLocation, Locatable, Located, RelativeLocation},
    parse_identifier,
    precedence::Precedence,
};

type ChildNode<L> = Located<Box<Node<L>>, L>;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node<L> {
    /// a | b
    BinaryOr(ChildNode<L>, ChildNode<L>),

    /// a & b
    BinaryAnd(ChildNode<L>, ChildNode<L>),

    /// a << b
    LeftShift(ChildNode<L>, ChildNode<L>),

    /// a >> b
    RightShift(ChildNode<L>, ChildNode<L>),

    /// a + b
    Sum(ChildNode<L>, ChildNode<L>),

    /// a - b
    Substract(ChildNode<L>, ChildNode<L>),

    /// a * b
    Multiply(ChildNode<L>, ChildNode<L>),

    /// a / b
    Divide(ChildNode<L>, ChildNode<L>),

    /// -a
    Invert(ChildNode<L>),

    /// ~a
    BinaryNot(ChildNode<L>),

    /// A literal value
    Literal(Value),

    /// A reference to a variable
    Variable(String),
}

impl<L> std::fmt::Display for Node<L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use Node::*;
        if f.sign_plus() {
            // Special case for indexed arguments
            match self {
                Invert(a) => write!(f, "- {}", a.inner),
                n => write!(f, "+ {}", n),
            }
        } else {
            match self {
                BinaryOr(a, b) => write!(
                    f,
                    "{} | {}",
                    a.inner.with_parent(self),
                    b.inner.with_parent(self)
                ),
                BinaryAnd(a, b) => write!(
                    f,
                    "{} & {}",
                    a.inner.with_parent(self),
                    b.inner.with_parent(self)
                ),
                LeftShift(a, b) => write!(
                    f,
                    "{} << {}",
                    a.inner.with_parent(self),
                    b.inner.with_parent(self)
                ),
                RightShift(a, b) => write!(
                    f,
                    "{} >> {}",
                    a.inner.with_parent(self),
                    b.inner.with_parent(self)
                ),
                Sum(a, b) => write!(
                    f,
                    "{} + {}",
                    a.inner.with_parent(self),
                    b.inner.with_parent(self)
                ),
                Substract(a, b) => write!(
                    f,
                    "{} - {}",
                    a.inner.with_parent(self),
                    b.inner.with_parent(self)
                ),
                Multiply(a, b) => write!(
                    f,
                    "{} * {}",
                    a.inner.with_parent(self),
                    b.inner.with_parent(self)
                ),
                Divide(a, b) => write!(
                    f,
                    "{} / {}",
                    a.inner.with_parent(self),
                    b.inner.with_parent(self)
                ),
                Invert(a) => write!(f, "-{}", a.inner.with_parent(self)),
                BinaryNot(a) => write!(f, "~{}", a.inner.with_parent(self)),
                Literal(a) => write!(f, "{}", a),
                Variable(a) => write!(f, "{}", a),
            }
        }
    }
}

impl Node<RelativeLocation> {
    #[allow(dead_code)]
    fn offset(self, offset: usize) -> Self {
        use Node::*;
        match self {
            BinaryOr(a, b) => BinaryOr(a.offset(offset), b.offset(offset)),
            BinaryAnd(a, b) => BinaryAnd(a.offset(offset), b.offset(offset)),
            LeftShift(a, b) => LeftShift(a.offset(offset), b.offset(offset)),
            RightShift(a, b) => RightShift(a.offset(offset), b.offset(offset)),
            Sum(a, b) => Sum(a.offset(offset), b.offset(offset)),
            Substract(a, b) => Substract(a.offset(offset), b.offset(offset)),
            Multiply(a, b) => Multiply(a.offset(offset), b.offset(offset)),
            Divide(a, b) => Divide(a.offset(offset), b.offset(offset)),
            Invert(a) => Invert(a.offset(offset)),
            BinaryNot(a) => BinaryNot(a.offset(offset)),
            Literal(a) => Literal(a),
            Variable(a) => Variable(a),
        }
    }

    pub(crate) fn into_absolute(self, location: &AbsoluteLocation) -> Node<AbsoluteLocation> {
        use Node::*;

        let mapper = |node: Box<Node<RelativeLocation>>, parent: &AbsoluteLocation| {
            Box::new(node.into_absolute(parent))
        };

        match self {
            BinaryOr(a, b) => BinaryOr(
                a.into_absolute(location, mapper),
                b.into_absolute(location, mapper),
            ),
            BinaryAnd(a, b) => BinaryAnd(
                a.into_absolute(location, mapper),
                b.into_absolute(location, mapper),
            ),
            LeftShift(a, b) => LeftShift(
                a.into_absolute(location, mapper),
                b.into_absolute(location, mapper),
            ),
            RightShift(a, b) => RightShift(
                a.into_absolute(location, mapper),
                b.into_absolute(location, mapper),
            ),
            Sum(a, b) => Sum(
                a.into_absolute(location, mapper),
                b.into_absolute(location, mapper),
            ),
            Substract(a, b) => Substract(
                a.into_absolute(location, mapper),
                b.into_absolute(location, mapper),
            ),
            Multiply(a, b) => Multiply(
                a.into_absolute(location, mapper),
                b.into_absolute(location, mapper),
            ),
            Divide(a, b) => Divide(
                a.into_absolute(location, mapper),
                b.into_absolute(location, mapper),
            ),
            Invert(a) => Invert(a.into_absolute(location, mapper)),
            BinaryNot(a) => BinaryNot(a.into_absolute(location, mapper)),
            Literal(a) => Literal(a),
            Variable(a) => Variable(a),
        }
    }
}

pub trait Context {
    // TODO: use something else than Value
    fn resolve_variable(&self, variable: &str) -> Option<Value>;
}

pub(crate) struct EmptyContext;
impl Context for EmptyContext {
    fn resolve_variable(&self, _variable: &str) -> Option<Value> {
        None
    }
}

#[derive(Error, Debug, PartialEq)]
pub enum EvaluationError {
    #[error("undefined variable {variable:?}")]
    UndefinedVariable { variable: String },

    #[error("could not downcast value")]
    Downcast,
}

impl<L> Node<L> {
    pub fn evaluate<C: Context, V: TryFrom<Value>>(
        &self,
        context: &C,
    ) -> Result<V, EvaluationError> {
        let value: Value =
            match self {
                Node::BinaryOr(left, right) => {
                    let left: Value = left.inner.evaluate(context)?;
                    let right: Value = right.inner.evaluate(context)?;
                    left | right
                }

                Node::BinaryAnd(left, right) => {
                    let left: Value = left.inner.evaluate(context)?;
                    let right: Value = right.inner.evaluate(context)?;
                    left & right
                }

                Node::LeftShift(left, right) => {
                    let left: Value = left.inner.evaluate(context)?;
                    let right: Value = right.inner.evaluate(context)?;
                    left << right
                }

                Node::RightShift(left, right) => {
                    let left: Value = left.inner.evaluate(context)?;
                    let right: Value = right.inner.evaluate(context)?;
                    left >> right
                }

                Node::Sum(left, right) => {
                    let left: Value = left.inner.evaluate(context)?;
                    let right: Value = right.inner.evaluate(context)?;
                    left + right
                }

                Node::Substract(left, right) => {
                    let left: Value = left.inner.evaluate(context)?;
                    let right: Value = right.inner.evaluate(context)?;
                    left - right
                }

                Node::Multiply(left, right) => {
                    let left: Value = left.inner.evaluate(context)?;
                    let right: Value = right.inner.evaluate(context)?;
                    left * right
                }

                Node::Divide(left, right) => {
                    let left: Value = left.inner.evaluate(context)?;
                    let right: Value = right.inner.evaluate(context)?;
                    left / right
                }

                Node::Invert(operand) => {
                    let operand: Value = operand.inner.evaluate(context)?;
                    -operand
                }

                Node::BinaryNot(operand) => {
                    let _operand: Value = operand.inner.evaluate(context)?;
                    // TODO: bit inversion is tricky because we're not supposed to know the word length
                    // here. It's a bit opiniated, but for now it tries casting down to u16 before
                    // negating.

                    /*
                    u16::try_from(v) // try casting it down to u16
                        .map(|v| !v) // invert the bits
                        .map(|v| v as _) // cast it back up
                    */
                    todo!()
                }

                Node::Literal(value) => *value,

                Node::Variable(variable) => context.resolve_variable(variable).ok_or(
                    EvaluationError::UndefinedVariable {
                        variable: variable.clone(),
                    },
                )?,
            };

        V::try_from(value).map_err(|_| EvaluationError::Downcast)
    }
}

/// The type of value used throughout the calculation
pub type Value = i128;

#[doc(hidden)]
fn parse_or_rec(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    let (input, _) = space0(input)?;
    let (input, _) = char('|')(input)?;
    let (input, _) = space0(input)?;
    parse_and(input)
}

/// Parse a bitwise "or" operation
fn parse_or(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    let (input, value) = parse_and(input)?;
    fold_many0(parse_or_rec, value, |value, arg| {
        Node::BinaryOr(
            Box::new(value).with_location(()),
            Box::new(arg).with_location(()),
        )
    })(input)
}

#[doc(hidden)]
fn parse_and_rec(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    let (input, _) = space0(input)?;
    let (input, _) = char('&')(input)?;
    let (input, _) = space0(input)?;
    parse_shift(input)
}

/// Parse a bitwise "and" operation
fn parse_and(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    let (input, value) = parse_shift(input)?;
    fold_many0(parse_and_rec, value, |value, arg| {
        Node::BinaryAnd(
            Box::new(value).with_location(()),
            Box::new(arg).with_location(()),
        )
    })(input)
}

/// Represents a bit-shift operation direction
#[derive(Clone)]
enum ShiftOp {
    /// Shift to the right (`>>`)
    Right,
    /// Shift to the left (`<<`)
    Left,
}

#[doc(hidden)]
fn parse_shift_rec(input: &str) -> IResult<&str, (ShiftOp, Node<RelativeLocation>)> {
    let (input, _) = space0(input)?;
    let (input, op) = alt((
        value(ShiftOp::Right, tag(">>")),
        value(ShiftOp::Left, tag("<<")),
    ))(input)?;
    let (input, _) = space0(input)?;
    let (input, value) = parse_sum(input)?;
    Ok((input, (op, value)))
}

/// Parse a bitshift operation
fn parse_shift(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    let (input, value) = parse_sum(input)?;
    let (input, op) = opt(parse_shift_rec)(input)?;

    let value = match op {
        Some((ShiftOp::Right, arg)) => Node::RightShift(
            Box::new(value).with_location(()),
            Box::new(arg).with_location(()),
        ),
        Some((ShiftOp::Left, arg)) => Node::LeftShift(
            Box::new(value).with_location(()),
            Box::new(arg).with_location(()),
        ),
        None => value,
    };

    Ok((input, value))
}

/// Represents a sum/sub operation
#[derive(Clone)]
enum SumOp {
    Sum,
    Sub,
}

#[doc(hidden)]
fn parse_sum_rec(input: &str) -> IResult<&str, (SumOp, Node<RelativeLocation>)> {
    let (input, _) = space0(input)?;
    let (input, op) = alt((
        value(SumOp::Sum, char('+')), // Add
        value(SumOp::Sub, char('-')), // Substract
    ))(input)?;
    let (input, _) = space0(input)?;
    let (input, value) = parse_mul(input)?;
    Ok((input, (op, value)))
}

/// Parse a sum/sub operation
fn parse_sum(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    let (input, value) = parse_mul(input)?;
    fold_many0(parse_sum_rec, value, |value, (op, arg)| match op {
        SumOp::Sum => Node::Sum(
            Box::new(value).with_location(()),
            Box::new(arg).with_location(()),
        ),
        SumOp::Sub => Node::Substract(
            Box::new(value).with_location(()),
            Box::new(arg).with_location(()),
        ),
    })(input)
}

/// Represents a multiply/divide operation
#[derive(Clone)]
enum MulOp {
    Mul,
    Div,
}

#[doc(hidden)]
fn parse_mul_rec(input: &str) -> IResult<&str, (MulOp, Node<RelativeLocation>)> {
    let (input, _) = space0(input)?;
    let (input, op) = alt((
        value(MulOp::Mul, char('*')), // Multiply
        value(MulOp::Div, char('/')), // Divide
    ))(input)?;
    let (input, _) = space0(input)?;
    let (input, value) = parse_unary(input)?;
    Ok((input, (op, value)))
}

/// Parse a multiply/divide operation
fn parse_mul(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    let (input, value) = parse_unary(input)?;
    fold_many0(parse_mul_rec, value, |value, (op, arg)| match op {
        MulOp::Mul => Node::Multiply(
            Box::new(value).with_location(()),
            Box::new(arg).with_location(()),
        ),
        MulOp::Div => Node::Divide(
            Box::new(value).with_location(()),
            Box::new(arg).with_location(()),
        ),
    })(input)
}

/// Parse unary operations (negation and bit inversion)
fn parse_unary(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    let (input, _) = space0(input)?;
    alt((
        map(preceded(char('-'), parse_atom), |n| {
            Node::Invert(Box::new(n).with_location(()))
        }),
        map(preceded(char('~'), parse_atom), |n| {
            Node::BinaryNot(Box::new(n).with_location(()))
        }),
        parse_atom,
    ))(input)
}

/// Parse an atom of an expression: either a literal or a full expression within parenthesis
fn parse_atom(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    let (input, _) = space0(input)?;
    alt((
        map(parse_number_literal, |v| Node::Literal(v as Value)),
        map(parse_identifier, |i| Node::Variable(i.into())),
        parse_parenthesis,
    ))(input)
}

/// Parse an expression surrounded by parenthesis
fn parse_parenthesis(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    let (input, _) = char('(')(input)?;
    let (input, _) = space0(input)?;
    let (input, value) = parse_or(input)?;
    let (input, _) = space0(input)?;
    let (input, _) = char(')')(input)?;
    Ok((input, value))
}

/// Parse an expression, returning its AST
pub(crate) fn parse_expression(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    parse_or(input)
}

#[cfg(test)]
mod tests {
    use nom::Finish;

    use super::*;

    #[track_caller]
    fn evaluate<L>(res: IResult<&str, Node<L>>) -> i128 {
        let (rest, node) = res.finish().unwrap();
        assert_eq!(rest, "");
        node.evaluate(&EmptyContext).unwrap()
    }

    #[test]
    fn calculation_test() {
        assert_eq!(evaluate(parse_expression("1 + 2")), 3);
        assert_eq!(evaluate(parse_expression("-3")), -3);
        assert_eq!(evaluate(parse_expression("5+2 * 3")), 11);
        assert_eq!(evaluate(parse_expression("(5 + 2) * 3")), 21);
        assert_eq!(evaluate(parse_expression("0xFF * 2")), 0x1FE);
        assert_eq!(evaluate(parse_expression("0x0F <<4")), 0xF0);
        assert_eq!(evaluate(parse_expression("0xF0>> 4")), 0x0F);
        assert_eq!(evaluate(parse_expression("0xAF & 0xF0")), 0xA0);
        assert_eq!(evaluate(parse_expression("0x0F | 0xF0")), 0xFF);
    }
}
