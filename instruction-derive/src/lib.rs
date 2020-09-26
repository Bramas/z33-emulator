extern crate proc_macro;

use proc_macro2::{Ident, Literal, Span, TokenStream};
use proc_macro_error::{abort, proc_macro_error};
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{parse::Result, Data, DataEnum, DeriveInput, Type};

// TODO: use appropriate span in quotes

#[proc_macro_derive(Instruction, attributes(labelable))]
#[proc_macro_error]
pub fn derive_instruction(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    // Parse the string representation
    let input: DeriveInput = syn::parse(input).unwrap();

    // Build the impl
    let gen = impl_instruction(&input).unwrap();

    // Return the generated impl
    gen.into()
}

fn impl_instruction(ast: &DeriveInput) -> Result<TokenStream> {
    let t = ast.ident.clone();

    let instructions = if let Data::Enum(ref e) = ast.data {
        parse_instructions(e)?
    } else {
        abort!(ast.span(), "should be only called on enums")
    };

    let display_impl = generate_display_trait(&t, &instructions);
    let parsers = generate_parsers(&instructions);
    let label_resolver = generate_label_resolver(&instructions);

    let im = quote! {
        #display_impl

        impl #t {
            #parsers

            #label_resolver
        }
    };

    Ok(im)
}

#[derive(Debug)]
struct Instruction {
    ident: Ident,
    args: Vec<Type>,
    labelable: Option<usize>,
}

/// Parse the list of instructions from an enum AST
fn parse_instructions(e: &DataEnum) -> Result<Vec<Instruction>> {
    let mut instructions = Vec::new();

    for variant in e.variants.iter() {
        // Let's parse the fields
        let (args, labelable) = match variant.fields {
            syn::Fields::Named(_) => {
                abort!(variant.fields.span(), "Named fields are not supported");
            }
            syn::Fields::Unnamed(ref fields) => {
                let args = fields.unnamed.iter().map(|f| f.ty.clone()).collect();
                // TODO: warn if multiple labelable are set
                let labelable = fields.unnamed.iter().position(|f| {
                    f.attrs.iter().any(|attr| {
                        attr.path
                            .get_ident()
                            .filter(|ident| *ident == "labelable")
                            .is_some()
                    })
                });
                (args, labelable)
            }
            syn::Fields::Unit => (Vec::new(), None),
        };

        instructions.push(Instruction {
            ident: variant.ident.clone(),
            args,
            labelable,
        });
    }

    Ok(instructions)
}

/// Generate the `Display` trait for the enum
fn generate_display_trait(ident: &Ident, instructions: &[Instruction]) -> TokenStream {
    let match_display = instructions.iter().fold(quote!(), |acc, instruction| {
        let t = instruction.ident.clone();
        let inst = t.to_string().to_lowercase();
        let arm = if instruction.args.is_empty() {
            let inst = Literal::string(&inst);
            quote_spanned! { Span::call_site() =>
                Self::#t => write!(f, #inst)
            }
        } else {
            let pat = instruction
                .args
                .iter()
                .enumerate()
                .fold(quote! {}, |pat, (i, arg)| {
                    let id = Ident::new(format!("arg{}", i).as_str(), arg.span());
                    quote! { #pat #id, }
                });

            let parts: Vec<_> = std::iter::repeat("{}")
                .take(instruction.args.len())
                .collect();
            let literal = format!("{} {}", inst, parts.join(", "));
            let literal = Literal::string(&literal);

            quote_spanned! { Span::call_site() =>
                Self::#t(#pat) => write!(f, #literal, #pat)
            }
        };

        quote_spanned! { Span::call_site() =>
            #arm,
            #acc
        }
    });

    quote! {
        impl ::std::fmt::Display for #ident {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                match self {
                    #match_display
                }
            }
        }
    }
}

/// Generates parsers for each instruction
fn generate_parsers(instructions: &[Instruction]) -> TokenStream {
    let (parser_alt, parser_funcs) = instructions.iter().fold((quote!(), quote!()), |(alt, funcs), instruction| {
        let t = instruction.ident.clone();
        let inst = t.to_string().to_lowercase();
        let func_name = Ident::new(format!("parse_{}_args", inst).as_str(), t.span());
        let inst = Literal::string(&inst);

        let parser = if instruction.args.is_empty() {
            quote_spanned! { Span::call_site() =>
                fn #func_name(input: &str) -> ::nom::IResult<&str, (Option<&str>, Self)> {
                    Ok((input, (None, Self::#t)))
                }
            }
        } else {
            let (body, pat) = instruction.args.iter().enumerate().fold(
                (quote!(), quote!()),
                |(body, pat), (i, arg)| {
                    let id = Ident::new(format!("arg{}", i).as_str(), arg.span());

                    let body = if i > 0 {
                        quote_spanned! { Span::call_site() =>
                            #body
                            let (input, _) = ::nom::character::complete::char(',')(input)?;
                            let (input, _) = ::nom::character::complete::space0(input)?;
                        }
                    } else {
                        body
                    };

                    let body = if Some(i) == instruction.labelable {
                        quote_spanned! { Span::call_site() =>
                            #body
                            let (input, (label, #id)) = crate::processor::Parsable::parse_labelable(input)?;
                            tracing::trace!("parsed arg {:?}", #id);
                            let (input, _) = ::nom::character::complete::space0(input)?;
                        }
                    } else {
                        quote_spanned! { Span::call_site() =>
                            #body
                            let (input, #id) = crate::processor::Parsable::parse(input)?;
                            tracing::trace!("parsed arg {:?}", #id);
                            let (input, _) = ::nom::character::complete::space0(input)?;
                        }
                    };
                    let pat = quote! { #pat #id, };
                    (body, pat)
                },
            );

            quote_spanned! { Span::call_site() =>
                #[tracing::instrument]
                fn #func_name(input: &str) -> ::nom::IResult<&str, (Option<&str>, Self)> {
                    let (input, _) = ::nom::character::complete::space1(input)?;
                    let label: Option<&str> = None;
                    #body
                    Ok((input, (label, Self::#t(#pat))))
                }
            }
        };

        let alt = quote_spanned! { Span::call_site() =>
            #alt
            #inst => ::nom::combinator::cut(Self::#func_name)(input),
        };

        let funcs = quote! {
            #parser
            #funcs
        };

        (alt, funcs)
    });

    quote! {
        #[tracing::instrument]
        pub fn parse(input: &str) -> ::nom::IResult<&str, (Option<&str>, Self)> {
            let original_input = input;
            let (input, inst) = ::nom::character::complete::alpha1(input)?;
            let inst = inst.to_string().to_ascii_lowercase();
            match inst.as_str() {
                #parser_alt
                _ => Err(::nom::Err::Error(::nom::error::make_error(original_input, ::nom::error::ErrorKind::Alt))),
            }
        }

        #parser_funcs
    }
}

/// Generates the `resolve_label` method that helps replacing labels with addresses.
fn generate_label_resolver(instructions: &[Instruction]) -> TokenStream {
    let match_label = instructions.iter().fold(quote!(), |acc, instruction| {
        let t = instruction.ident.clone();
        let arm = if instruction.args.is_empty() {
            quote_spanned! { Span::call_site() =>
                Self::#t => None
            }
        } else if let Some(labelable) = instruction.labelable {
            let pat = instruction
                .args
                .iter()
                .enumerate()
                .fold(quote! {}, |pat, (i, arg)| {
                    let id = Ident::new(format!("arg{}", i).as_str(), arg.span());
                    quote! { #pat #id, }
                });

            let labelable = Ident::new(format!("arg{}", labelable).as_str(), t.span());

            quote_spanned! { Span::call_site() =>
                Self::#t(#pat) => {
                    let #labelable = #labelable.resolve_label(address)?;
                    Some(Self::#t(#pat))
                }
            }
        } else {
            quote_spanned! { Span::call_site() =>
                Self::#t(..) => None
            }
        };

        quote_spanned! { Span::call_site() =>
            #arm,
            #acc
        }
    });

    quote! {
        /// Change the label in an instruction by the given address.
        ///
        /// Some instructions can't have labels in them, in that case this method returns `None`.
        pub fn resolve_label(self, address: u64) -> Option<Self> {
            match self {
                #match_label
            }
        }
    }
}
