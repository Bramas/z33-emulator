//! Program line parsing
//!
//! This module parses whole program lines, including the symbol definitions, the comments and the
//! line content itself (either an instruction or a directive).
//!
//! When parsing, this module does zero copy over the original input. All members of resulting Line
//! structure reference part of the input, hence the associated lifetime on the structure tied to
//! the original input. This allows some neat tricks, especially calculating the offset of
//! a property from the input string.

use nom::{
    branch::alt,
    bytes::complete::{escaped, tag},
    character::complete::{char, line_ending, none_of, not_line_ending, one_of, space0, space1},
    combinator::{all_consuming, eof, opt, peek, value},
    multi::{many0, separated_list1},
    sequence::{delimited, preceded, terminated},
    IResult,
};

use super::{
    parse_identifier,
    value::{
        parse_directive_argument, parse_directive_kind, parse_instruction_argument,
        parse_instruction_kind, DirectiveArgument, DirectiveKind, InstructionArgument,
        InstructionKind,
    },
};

/// Holds the content of a line
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum LineContent {
    /// Represents an instruction, with its opcode and list of arguments
    Instruction {
        kind: InstructionKind,
        arguments: Vec<InstructionArgument>,
    },
    /// Represents a directive, with its type and argument
    Directive {
        kind: DirectiveKind,
        argument: DirectiveArgument,
    },
}

impl LineContent {
    /// Check if the line is a directive
    pub(crate) fn is_directive(&self) -> bool {
        matches!(self, Self::Directive { .. })
    }
}

impl std::fmt::Display for LineContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LineContent::Instruction { kind, arguments } => {
                // First write the opcode
                write!(f, "{:4}", kind)?;

                // then the list of arguments
                let mut first = true; // This is to properly show comma between arguments
                for arg in arguments.iter() {
                    if !first {
                        write!(f, ",")?;
                    }
                    write!(f, " {}", arg)?;
                    first = false;
                }
                Ok(())
            }
            LineContent::Directive {
                kind: directive,
                argument,
            } => {
                write!(f, ".{}: {}", directive, argument)
            }
        }
    }
}

/// Holds a whole line, with the symbol definitions (if any), the content (if any) and the comment
/// (if any).
///
/// Note that the `Default::default()` implementation represents an empty line.
#[derive(Debug, PartialEq, Default)]
pub(crate) struct Line {
    pub symbols: Vec<String>,
    pub content: Option<LineContent>,
    comment: Option<String>,
}

impl std::fmt::Display for Line {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut had_something = false;
        for symbol in self.symbols.iter() {
            write!(f, "{}: ", symbol)?;
            had_something = true;
        }

        if let Some(ref c) = self.content {
            if !c.is_directive() && !had_something {
                write!(f, "    ")?;
            }
            write!(f, "{}", c)?;
            had_something = true;
        }

        if let Some(ref c) = self.comment {
            if had_something {
                write!(f, "\t{}", c)?;
            } else {
                write!(f, "{}", c)?;
            }
        }

        Ok(())
    }
}

impl Line {
    #[cfg(test)] // Only used in tests for now
    pub(crate) fn comment(mut self, comment: &str) -> Self {
        self.comment = Some(comment.into());
        self
    }

    #[cfg(test)] // Only used in tests for now
    pub(crate) fn symbol(mut self, symbol: &str) -> Self {
        self.symbols.push(symbol.into());
        self
    }

    #[cfg(test)] // Only used in tests for now
    pub(crate) fn directive<T: Into<DirectiveArgument>>(
        mut self,
        kind: DirectiveKind,
        argument: T,
    ) -> Self {
        self.content = Some(LineContent::Directive {
            kind,
            argument: argument.into(),
        });
        self
    }

    #[cfg(test)] // Only used in tests for now
    pub(crate) fn instruction(
        mut self,
        kind: InstructionKind,
        arguments: Vec<InstructionArgument>,
    ) -> Self {
        self.content = Some(LineContent::Instruction { kind, arguments });
        self
    }
}

/// Parses a directive
fn parse_directive_line(input: &str) -> IResult<&str, LineContent> {
    let (input, _) = char('.')(input)?;
    let (input, kind) = parse_directive_kind(input)?;
    let (input, _) = space1(input)?;
    let (input, argument) = parse_directive_argument(input)?;
    Ok((input, LineContent::Directive { kind, argument }))
}

/// Parses an instruction
fn parse_instruction_line(input: &str) -> IResult<&str, LineContent> {
    let (input, kind) = parse_instruction_kind(input)?;
    let (input, arguments) = opt(preceded(
        space1,
        separated_list1(
            delimited(space0, char(','), space0),
            parse_instruction_argument,
        ),
    ))(input)?;
    let arguments = arguments.unwrap_or_default();
    Ok((input, LineContent::Instruction { kind, arguments }))
}

/// Parses the content of a line: an instruction or a directive
fn parse_line_content(input: &str) -> IResult<&str, LineContent> {
    alt((parse_directive_line, parse_instruction_line))(input)
}

/// Parses an inline comment
fn parse_comment(input: &str) -> IResult<&str, String> {
    let (input, _) = peek(tag("#"))(input)?;
    let (input, comment) = not_line_ending(input)?;
    Ok((input, comment.into()))
}

/// Parses symbol definitions
fn parse_symbol_definition(input: &str) -> IResult<&str, String> {
    let (input, symbol) = parse_identifier(input)?;
    let (input, _) = space0(input)?;
    let (input, _) = char(':')(input)?;
    Ok((input, symbol.into()))
}

/// Parses a whole line
fn parse_line(input: &str) -> IResult<&str, Line> {
    let (input, _) = space0(input)?;

    // Extract the list of symbol definitions
    let (input, symbols) = many0(terminated(parse_symbol_definition, space0))(input)?;
    let (input, _) = space0(input)?;

    // Extract the line content
    let (input, content) = opt(parse_line_content)(input)?;
    let (input, _) = space0(input)?;

    // Extract the comment
    let (input, comment) = opt(parse_comment)(input)?;

    // Build the line
    Ok((
        input,
        Line {
            symbols,
            content,
            comment,
        },
    ))
}

fn split_lines(input: &str) -> IResult<&str, Vec<&str>> {
    let line_parser = escaped(none_of("\\\r\n"), '\\', one_of("\\\r\nrnt\""));
    let line_parser = alt((line_parser, eof, value("", peek(line_ending))));
    separated_list1(line_ending, line_parser)(input)
}

pub(crate) fn parse_program(input: &str) -> IResult<&str, Vec<Line>> {
    let (input, lines) = split_lines(input)?;
    let lines: Result<_, _> = lines
        .into_iter()
        .map(|line| all_consuming(parse_line)(line).map(|(_, line)| line))
        .collect();
    Ok((input, lines?))
}

#[cfg(test)]
mod tests {
    use crate::runtime::Reg;

    use super::super::location::Locatable;
    use super::*;

    #[track_caller]
    fn fully_parsed<T>(result: IResult<&str, T>) -> T {
        let (input, result) = result.unwrap();
        assert_eq!(input, "");
        result
    }

    #[test]
    fn parse_empty_line_test() {
        let line = fully_parsed(parse_line(""));
        assert_eq!(line, Line::default());
    }

    #[test]
    fn parse_comment_line_test() {
        let line = fully_parsed(parse_line("# hello"));
        assert_eq!(
            line,
            Line {
                comment: Some("# hello".into()),
                ..Default::default()
            }
        );
    }

    #[test]
    fn parse_symbol_line_test() {
        let line = fully_parsed(parse_line("hello:world: duplicate: duplicate:  "));
        assert_eq!(
            line,
            Line {
                symbols: ["hello", "world", "duplicate", "duplicate"]
                    .iter()
                    .cloned()
                    .map(Into::into)
                    .collect(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn parse_full_line_test() {
        use super::super::expression::Node;
        let line = fully_parsed(parse_line("foo: bar: .space 30 + 5 # comment"));
        assert_eq!(
            line,
            Line {
                symbols: ["foo", "bar"].iter().cloned().map(Into::into).collect(),
                content: Some(LineContent::Directive {
                    kind: DirectiveKind::Space,
                    argument: DirectiveArgument::Expression(Node::Sum(
                        Box::new(Node::Literal(30)).with_location(()),
                        Box::new(Node::Literal(5)).with_location(())
                    )),
                }),
                comment: Some("# comment".into()),
            }
        );
    }

    #[test]
    fn split_lines_test() {
        let input = r#"hello \
world
this is a new line
this has escaped chars: \r \n \t \""#;
        let lines = fully_parsed(split_lines(input));
        assert_eq!(
            lines,
            vec![
                "hello \\\nworld",
                "this is a new line",
                "this has escaped chars: \\r \\n \\t \\\""
            ]
        );
    }

    #[test]
    fn parse_program_test() {
        use DirectiveKind::Space;
        use InstructionKind::Add;

        let input = r#"
            str: .space "some multiline \
string"
            main: # beginning of program
                add %a, %b
        "#;

        let lines = fully_parsed(parse_program(input));
        assert_eq!(
            lines,
            vec![
                Line::default(),
                Line::default().symbol("str").directive(
                    Space,
                    DirectiveArgument::StringLiteral("some multiline string".into())
                ),
                Line::default()
                    .symbol("main")
                    .comment("# beginning of program"),
                Line::default().instruction(
                    Add,
                    vec![
                        InstructionArgument::Register(Reg::A),
                        InstructionArgument::Register(Reg::B)
                    ]
                ),
                Line::default(),
            ]
        );
    }
}
