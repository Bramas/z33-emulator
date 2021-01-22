use nom::{
    branch::alt,
    bytes::complete::{tag, take_till},
    character::complete::{char, line_ending, not_line_ending, space0, space1},
    combinator::{map, not, opt},
    sequence::preceded,
    IResult, Offset,
};

use super::{
    literal::parse_string_literal,
    location::{AbsoluteLocation, Locatable, Located, RelativeLocation},
    parse_identifier,
};

type Children<L> = Vec<Located<Node<L>, L>>;

#[derive(Debug, PartialEq)]
pub(crate) struct ConditionBranch<L> {
    pub condition: Located<String, L>,
    pub body: Located<Children<L>, L>,
}

impl ConditionBranch<RelativeLocation> {
    fn into_absolute(self, parent: &AbsoluteLocation) -> ConditionBranch<AbsoluteLocation> {
        let condition = self.condition.into_absolute(parent, |c, _| c);
        let body = self.body.into_absolute(parent, |c, p| {
            c.into_iter()
                .map(|c| c.into_absolute(p, |c, p| c.into_absolute(p)))
                .collect()
        });
        ConditionBranch { condition, body }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum Node<L> {
    Raw {
        content: String,
    },
    Error {
        message: Located<String, L>,
    },
    Undefine {
        key: Located<String, L>,
    },
    Definition {
        key: Located<String, L>,
        content: Option<Located<String, L>>,
    },
    Inclusion {
        path: Located<String, L>,
    },
    Condition {
        branches: Vec<ConditionBranch<L>>,
        fallback: Option<Located<Children<L>, L>>,
    },
}

impl<L> Node<L> {
    pub(crate) fn walk<F>(&self, f: &mut F)
    where
        F: FnMut(&Self),
    {
        f(self);

        if let Node::Condition { branches, fallback } = self {
            for branch in branches.iter() {
                for chunk in branch.body.inner.iter() {
                    chunk.inner.walk(f)
                }
            }

            if let Some(fallback) = fallback {
                for chunk in fallback.inner.iter() {
                    chunk.inner.walk(f)
                }
            }
        }
    }
}

impl Node<RelativeLocation> {
    pub(crate) fn into_absolute(self, parent: &AbsoluteLocation) -> Node<AbsoluteLocation> {
        use Node::*;
        match self {
            Raw { content } => Raw { content },
            Error { message } => Error {
                message: message.into_absolute(&parent, |c, _| c),
            },
            Undefine { key } => Undefine {
                key: key.into_absolute(&parent, |k, _| k),
            },
            Definition { key, content } => Definition {
                key: key.into_absolute(&parent, |k, _| k),
                content: content.map(|c| c.into_absolute(&parent, |c, _| c)),
            },
            Inclusion { path } => Inclusion {
                path: path.into_absolute(&parent, |p, _| p),
            },
            Condition { branches, fallback } => Condition {
                branches: branches
                    .into_iter()
                    .map(|b| b.into_absolute(&parent))
                    .collect(),
                fallback: fallback.map(
                    |l: Located<Children<RelativeLocation>, RelativeLocation>| {
                        l.into_absolute(&parent, |c, p| {
                            c.into_iter()
                                .map(|c| c.into_absolute(p, |c, p| c.into_absolute(p)))
                                .collect()
                        })
                    },
                ),
            },
        }
    }
}

/// Eats the end of a line, including trailing spaces, inline comments and the line ending
fn eat_end_of_line(input: &str) -> IResult<&str, ()> {
    let (rest, _) = space0(input)?;
    let (rest, _) = opt(preceded(tag("//"), not_line_ending))(rest)?;
    Ok((rest, ()))
}

/// Extracts the argument of a directive
/// It tries to stop before any trailing whitespace or comment
fn parse_directive_argument(input: &str) -> IResult<&str, &str> {
    let mut cursor = input;
    loop {
        let (rest, _) = space0(cursor)?;

        // peek at the next thing after the spaces
        match rest.get(..2) {
            Some("//") | Some("\r\n") => break,
            _ => {}
        }

        if let Some("\n") = rest.get(..1) {
            break;
        }

        if rest.is_empty() {
            break;
        }

        let (rest, _) =
            take_till(|c| c == ' ' || c == '\t' || c == '\r' || c == '\n' || c == '/')(rest)?;
        cursor = rest;
    }

    let index = input.offset(cursor);
    if index == 0 {
        return Err(nom::Err::Error(nom::error::ParseError::from_error_kind(
            input,
            nom::error::ErrorKind::Eof, // TODO: maybe not the best kind
        )));
    }

    let content = &input[..index];
    Ok((cursor, content))
}

fn parse_definition(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    let (rest, _) = char('#')(input)?;
    let (rest, _) = space0(rest)?;
    let (rest, _) = tag("define")(rest)?;
    let (rest, _) = space1(rest)?;
    let start = rest;
    let (rest, key) = parse_identifier(rest)?;
    let key = key.to_owned().with_location((input, start, rest));

    let (rest, content) = opt(|rest| {
        let (rest, _) = space1(rest)?;
        let start = rest;
        let (rest, content) = parse_directive_argument(rest)?;
        let content = content.to_owned().with_location((input, start, rest));
        Ok((rest, content))
    })(rest)?;

    let (rest, _) = eat_end_of_line(rest)?;

    Ok((rest, Node::Definition { key, content }))
}

fn parse_undefine(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    let (rest, _) = char('#')(input)?;
    let (rest, _) = space0(rest)?;
    let (rest, _) = tag("undefine")(rest)?;
    let (rest, _) = space1(rest)?;

    let start = rest;
    let (rest, key) = parse_identifier(rest)?;
    let key = key.to_owned().with_location((input, start, rest));

    let (rest, _) = eat_end_of_line(rest)?;

    Ok((rest, Node::Undefine { key }))
}

fn parse_inclusion(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    // Parse "#include"
    let (rest, _) = char('#')(input)?;
    let (rest, _) = space0(rest)?;
    let (rest, _) = tag("include")(rest)?;
    let (rest, _) = space1(rest)?;

    // Parse the argument
    let start = rest;
    let (rest, path) = parse_string_literal(rest)?;
    let path = path.with_location((input, start, rest));

    let (rest, _) = eat_end_of_line(rest)?;

    Ok((rest, Node::Inclusion { path }))
}

fn parse_error(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    // Parse "#error"
    let (rest, _) = char('#')(input)?;
    let (rest, _) = space0(rest)?;
    let (rest, _) = tag("error")(rest)?;
    let (rest, _) = space1(rest)?;

    // Parse the argument
    let start = rest;
    let (rest, message) = parse_string_literal(rest)?;
    let message = message.with_location((input, start, rest));

    let (rest, _) = eat_end_of_line(rest)?;

    Ok((rest, Node::Error { message }))
}

fn parse_condition(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    // First, get the "#if"
    let (rest, _) = char('#')(input)?;
    let (rest, _) = space0(rest)?;
    let (rest, _) = tag("if")(rest)?;
    let (rest, _) = space1(rest)?;

    // Then parse the first condition
    let start = rest;
    let (rest, condition) = parse_directive_argument(rest)?;
    let condition = condition.to_string().with_location((input, start, rest));

    let (rest, _) = eat_end_of_line(rest)?;
    let (rest, _) = line_ending(rest)?;

    // Parse its body
    let start = rest;
    let (rest, body) = parse(rest)?;
    let body = body.with_location((input, start, rest));

    // We have the first branch parsed, let's parse the others
    let branch = ConditionBranch { condition, body };

    // This structure helps parsing the else/elif/end directives
    enum BranchDirective {
        Else,
        ElseIf(Located<String, RelativeLocation>),
        EndIf,
    };
    use BranchDirective::*;

    let mut cursor = rest; // This saves current location in the input
    let mut branches = vec![branch];
    let mut fallback = None;

    loop {
        // First, get the "#if"
        let (rest, _) = char('#')(cursor)?;
        let (rest, _) = space0(rest)?;
        let (rest, directive) = alt((
            map(tag("endif"), |_| EndIf),
            |rest| {
                // Parse a "elif" directive
                let (rest, _) = tag("elif")(rest)?;
                let (rest, _) = space1(rest)?;
                let start = rest;
                let (rest, condition) = parse_directive_argument(rest)?;
                let condition = condition.to_string().with_location((input, start, rest));
                Ok((rest, ElseIf(condition)))
            },
            map(tag("else"), |_| Else),
        ))(rest)?;

        let (rest, _) = eat_end_of_line(rest)?;

        // We've got an "#end" directive, get out of the loop
        // We don't update the cursor here since we will be re-parsing the #end directive afterward
        if let EndIf = directive {
            break;
        }

        let (rest, _) = line_ending(rest)?;

        let start = rest;
        let (rest, body) = parse(rest)?;
        let body = body.with_location((input, start, rest));

        cursor = rest;

        if let ElseIf(condition) = directive {
            branches.push(ConditionBranch { condition, body })
        } else {
            fallback = Some(body);
            break;
        }
    }

    let (rest, _) = char('#')(cursor)?;
    let (rest, _) = space0(rest)?;
    let (rest, _) = tag("endif")(rest)?;

    let (rest, _) = eat_end_of_line(rest)?;

    Ok((rest, Node::Condition { branches, fallback }))
}

fn parse_raw(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    let (rest, _) = not(char('#'))(input)?;
    let (rest, content) = not_line_ending(rest)?;
    let content = content.to_string();
    Ok((rest, Node::Raw { content }))
}

fn parse_chunk(input: &str) -> IResult<&str, Node<RelativeLocation>> {
    alt((
        parse_definition, // #define X [Y]
        parse_undefine,   // #undefine X
        parse_inclusion,  // #include "X"
        parse_condition,  // #if X ... [#elif Y ...] [#else Z ...] #endif
        parse_error,      // #error "X"
        parse_raw,        // anything else
    ))(input)
}

pub(crate) fn parse(input: &str) -> IResult<&str, Children<RelativeLocation>> {
    let mut chunks = Vec::new();
    let mut cursor = input;

    while let Ok((rest, chunk)) = parse_chunk(cursor) {
        let chunk = chunk.with_location((input, cursor, rest));
        chunks.push(chunk);

        if let Ok((rest, _)) = line_ending::<_, nom::error::Error<_>>(rest) {
            cursor = rest;
        } else {
            cursor = rest;
            // Got no linebreak, get out of the loop
            break;
        }
    }

    Ok((cursor, chunks))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_directive_argument_test() {
        let res = parse_directive_argument("foo");
        assert_eq!(res, Ok(("", "foo")));
    }

    #[test]
    fn parse_definition_test() {
        let res = parse_definition("#define foo bar");
        assert_eq!(
            res,
            Ok((
                "",
                Node::Definition {
                    key: "foo".to_owned().with_location((8, 3)),
                    content: Some("bar".to_owned().with_location((12, 3))),
                }
            ))
        );

        let res = parse_definition("#define foo");
        assert_eq!(
            res,
            Ok((
                "",
                Node::Definition {
                    key: "foo".to_owned().with_location((8, 3)),
                    content: None,
                }
            ))
        );

        let res = parse_definition("#define trailing ");
        assert_eq!(
            res,
            Ok((
                "",
                Node::Definition {
                    key: "trailing".to_owned().with_location((8, 8)),
                    content: None,
                }
            ))
        );
    }

    #[test]
    fn parse_inclusion_test() {
        let res = parse_inclusion("#include \"foo\"");
        assert_eq!(
            res,
            Ok((
                "",
                Node::Inclusion {
                    path: "foo".to_string().with_location((9, 5))
                }
            ))
        );
    }

    #[test]
    fn parse_raw_test() {
        // It extracts the line
        let (rest, body) = parse_raw("line").unwrap();
        assert_eq!(rest, "");
        assert_eq!(
            body,
            Node::Raw {
                content: "line".to_string()
            }
        );

        // Gets only one line
        let (rest, body) = parse_raw("line\nline").unwrap();
        assert_eq!(rest, "\nline");
        assert_eq!(
            body,
            Node::Raw {
                content: "line".to_string()
            }
        );

        // Does not get directives
        assert!(parse_raw("#directive").is_err());
    }

    #[test]
    fn parse_simple_test() {
        use Node::*;
        let (rest, body) = parse(indoc::indoc! {r#"
            hello
            #include "foo"
            #define bar baz
            world
            #error "deprecated"
            #undefine bar
            #define test
        "#})
        .unwrap();
        assert_eq!(rest, "");

        assert_eq!(
            body,
            vec![
                Raw {
                    content: "hello".to_string()
                }
                .with_location((0, 5)),
                Inclusion {
                    path: "foo".to_string().with_location((9, 5))
                }
                .with_location((6, 14)),
                Definition {
                    key: "bar".to_string().with_location((8, 3)),
                    content: Some("baz".to_string().with_location((12, 3)))
                }
                .with_location((21, 15)),
                Raw {
                    content: "world".to_string()
                }
                .with_location((37, 5)),
                Error {
                    message: "deprecated".to_string().with_location((7, 12))
                }
                .with_location((43, 19)),
                Undefine {
                    key: "bar".to_string().with_location((10, 3)),
                }
                .with_location((63, 13)),
                Definition {
                    key: "test".to_string().with_location((8, 4)),
                    content: None,
                }
                .with_location((77, 12)),
                Raw {
                    content: "".to_string(),
                }
                .with_location((90, 0))
            ]
        );
    }

    #[test]
    fn parse_weird_test() {
        use Node::*;
        let (rest, body) = parse(indoc::indoc! {r#"
            hello
            #  include   "foo"//comment
            # define   bar   baz  // comment
            world
            #   error    "deprecated"
            # undefine  bar //comment
            #   define  test // comment

            empty line
        "#})
        .unwrap();
        assert_eq!(rest, "");

        assert_eq!(
            body,
            vec![
                Raw {
                    content: "hello".to_string()
                }
                .with_location((0, 5)),
                Inclusion {
                    path: "foo".to_string().with_location((13, 5))
                }
                .with_location((6, 27)),
                Definition {
                    key: "bar".to_string().with_location((11, 3)),
                    content: Some("baz".to_string().with_location((17, 3)))
                }
                .with_location((34, 32)),
                Raw {
                    content: "world".to_string()
                }
                .with_location((67, 5)),
                Error {
                    message: "deprecated".to_string().with_location((13, 12))
                }
                .with_location((73, 25)),
                Undefine {
                    key: "bar".to_string().with_location((12, 3)),
                }
                .with_location((99, 25)),
                Definition {
                    key: "test".to_string().with_location((12, 4)),
                    content: None,
                }
                .with_location((125, 27)),
                Raw {
                    content: "".to_string()
                }
                .with_location((153, 0)),
                Raw {
                    content: "empty line".to_string()
                }
                .with_location((154, 10)),
                Raw {
                    content: "".to_string()
                }
                .with_location((165, 0)),
            ]
        );
    }

    #[test]
    fn parse_condition_test() {
        use Node::*;
        let input = indoc::indoc! {r#"
            #if true
            foo
            #if false
            bar
            #endif
            #elif true // with a comment
            foobar
            #else
            baz
            #endif
        "#}
        .trim_end(); // Remove the trailing linebreak

        // Check a few locations used bellow
        assert_eq!(&input[4..8], "true"); // line 1
        assert_eq!(&input[9..12], "foo"); // line 2
        assert_eq!(&input[40..44], "true"); // line 6
        assert_eq!(&input[63..69], "foobar"); // line 7
        assert_eq!(&input[76..79], "baz"); // line 9

        let (rest, condition) = parse_condition(input).unwrap();

        assert_eq!(rest, "");
        assert_eq!(
            condition,
            Condition {
                branches: vec![
                    ConditionBranch {
                        condition: "true".to_string().with_location((4, 4)),
                        body: vec![
                            Raw {
                                content: "foo".to_string()
                            }
                            .with_location((0, 3)),
                            Condition {
                                branches: vec![ConditionBranch {
                                    condition: "false".to_string().with_location((4, 5)),
                                    body: vec![Raw {
                                        content: "bar".to_string()
                                    }
                                    .with_location((0, 3))]
                                    .with_location((10, 4))
                                },],
                                fallback: None,
                            }
                            .with_location((4, 20)),
                        ]
                        .with_location((9, 25))
                    },
                    ConditionBranch {
                        condition: "true".to_string().with_location((40, 4)),
                        body: vec![Raw {
                            content: "foobar".to_string()
                        }
                        .with_location((0, 6))]
                        .with_location((63, 7))
                    }
                ],
                fallback: Some(
                    vec![
                        // "else" content
                        Raw {
                            content: "baz".to_string()
                        }
                        .with_location((0, 3))
                    ]
                    .with_location((76, 4))
                )
            }
        );
    }
}