use crate::{
    args::Arguments,
    attributes::AttributeCollector,
    common::{into_proc_macro_result, with_parsed_values},
};
use cairo_lang_macro::{
    quote, Diagnostic, Diagnostics, ProcMacroResult, TextSpan, Token, TokenStream, TokenTree,
};
use cairo_lang_parser::utils::SimpleParserDatabase;
use cairo_lang_syntax::node::with_db::SyntaxNodeWithDb;
use cairo_lang_syntax::node::{
    ast::{Condition, Expr, FunctionWithBody, Statement},
    helpers::GetIdentifier,
    TypedSyntaxNode,
};

#[allow(clippy::needless_pass_by_value)]
pub fn extend_with_config_cheatcodes<Collector>(
    args: TokenStream,
    item: TokenStream,
) -> ProcMacroResult
where
    Collector: AttributeCollector,
{
    into_proc_macro_result(args, item, |args, item, warns| {
        with_parsed_values::<Collector>(args, item, warns, with_config_cheatcodes::<Collector>)
    })
}

fn with_config_cheatcodes<Collector>(
    db: &SimpleParserDatabase,
    func: &FunctionWithBody,
    args_db: &SimpleParserDatabase,
    args: Arguments,
    warns: &mut Vec<Diagnostic>,
) -> Result<TokenStream, Diagnostics>
where
    Collector: AttributeCollector,
{
    let value = Collector::args_into_config_expression(args_db, args, warns)?;

    let cheatcode_name = Collector::CHEATCODE_NAME;

    let cheatcode = TokenTree::Ident(Token::new(
        format!("starknet::testing::cheatcode::<'{cheatcode_name}'>(data.span())"),
        TextSpan::call_site(),
    ));

    let config_cheatcode = quote!(
            let mut data = array![];

            #value
            .serialize(ref data);

            #cheatcode;
    );

    Ok(append_config_statements(db, func, &config_cheatcode))
}

pub fn append_config_statements(
    db: &SimpleParserDatabase,
    func: &FunctionWithBody,
    config_statements: &TokenStream,
) -> TokenStream {
    let vis = func.visibility(db).as_syntax_node();
    let attrs = func.attributes(db).as_syntax_node();
    let declaration = func.declaration(db).as_syntax_node();
    let statements = func.body(db).statements(db).elements(db);

    let if_content = statements.first().and_then(|stmt| {
        // first statement is `if`
        let Statement::Expr(expr) = stmt else {
            return None;
        };
        let Expr::If(if_expr) = expr.expr(db) else {
            return None;
        };
        // it's condition is function call
        let Condition::Expr(expr) = if_expr.condition(db) else {
            return None;
        };
        let Expr::FunctionCall(expr) = expr.expr(db) else {
            return None;
        };

        // this function is named "snforge_std::_internals::_is_config_run"
        let segments = expr.path(db).elements(db);

        let [snforge_std, cheatcode, is_config_run] = segments.as_slice() else {
            return None;
        };

        if snforge_std.identifier(db) != "snforge_std"
            || cheatcode.identifier(db) != "_internals"
            || is_config_run.identifier(db) != "_is_config_run"
        {
            return None;
        }

        let statements = if_expr.if_block(db).statements(db).elements(db);

        // omit last one (`return;`) as it have to be inserted after all new statements
        Some(
            statements[..statements.len() - 1]
                .iter()
                // .fold(String::new(), |acc, statement| {
                //     acc + "\n" + &statement.as_syntax_node().get_text(db)
                // }),
                .map(|stmt| {
                    let syntax = stmt.as_syntax_node();
                    let syntax = SyntaxNodeWithDb::new(&syntax, db);
                    quote!(#syntax)
                })
                .fold(TokenStream::empty(), |mut acc, token| {
                    acc.extend(token);
                    acc
                }),
        )
    });

    // there was already config check, omit it and collect remaining statements
    let statements = if if_content.is_some() {
        &statements[1..]
    } else {
        &statements[..]
    }
    .iter()
    .map(|t| {
        let syntax = t.as_syntax_node();
        let syntax = SyntaxNodeWithDb::new(&syntax, db);
        quote!(#syntax)
    })
    .fold(TokenStream::empty(), |mut acc, token| {
        acc.extend(token);
        acc
    });

    let if_content = if_content.unwrap_or_else(TokenStream::empty);

    // let statements = statements;

    // .into_iter().map(|stmt| {
    //     let syntax = stmt.as_syntax_node();
    //     let syntax = SyntaxNodeWithDb::new(&syntax, db);
    //     let token = ToPrimitiveTokenStream::to_primitive_token_stream(&syntax);
    //     TokenStream::from_primitive_token_stream(token)
    // });

    // quote!(
    //         #attrs
    //         #vis #declaration {{
    //             if snforge_std::_internals::_is_config_run() {{
    //                 #if_content
    //
    //                 #config_statements
    //
    //                 return;
    //             }}
    //
    //             #statements
    //         }}
    // )

    let attrs = SyntaxNodeWithDb::new(&attrs, db);
    let vis = SyntaxNodeWithDb::new(&vis, db);
    let declaration = SyntaxNodeWithDb::new(&declaration, db);
    let config_statements = config_statements.clone();
    quote!(
            #attrs
            #vis #declaration {
                if snforge_std::_internals::_is_config_run() {
                    #if_content

                    #config_statements

                    return;
                }

                #statements
            }
    )
}
