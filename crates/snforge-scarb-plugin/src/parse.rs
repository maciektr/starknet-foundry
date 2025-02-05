use crate::attributes::{AttributeInfo, ErrorExt};
use cairo_lang_macro::{quote, Diagnostic, TextSpan, Token, TokenStream, TokenTree};
use cairo_lang_parser::utils::SimpleParserDatabase;
use cairo_lang_syntax::node::ast::SyntaxFile;
use cairo_lang_syntax::node::{
    ast::{FunctionWithBody, ModuleItem, OptionArgListParenthesized},
    helpers::QueryAttrs,
    TypedSyntaxNode,
};
use cairo_lang_utils::Upcast;

pub fn parse<T: AttributeInfo>(
    code: &TokenStream,
) -> Result<(SimpleParserDatabase, FunctionWithBody), Diagnostic> {
    let simple_db = SimpleParserDatabase::default();
    let (parsed_node, _diagnostics) = simple_db.parse_token_stream(code);

    let db: &SimpleParserDatabase = simple_db.upcast();
    let elements = SyntaxFile::from_syntax_node(db, parsed_node)
        .items(db)
        .elements(db);

    elements
        .into_iter()
        .find_map(|element| {
            if let ModuleItem::FreeFunction(func) = element {
                Some(func)
            } else {
                None
            }
        })
        .map(|func| (simple_db, func))
        .ok_or_else(|| T::error("can be used only on a function"))
}

struct InternalCollector;

impl AttributeInfo for InternalCollector {
    const ATTR_NAME: &'static str = "__SNFORGE_INTERNAL_ATTR__";
}

pub fn parse_args(args: &TokenStream) -> (SimpleParserDatabase, OptionArgListParenthesized) {
    let args = args.clone();
    let attr_name = TokenTree::Ident(Token::new(
        InternalCollector::ATTR_NAME,
        TextSpan::call_site(),
    ));
    let code = quote! {
        #[#attr_name #args]
        fn __SNFORGE_INTERNAL_FN__(){{}}
    };
    let (simple_db, func) = parse::<InternalCollector>(&code)
        .expect("Parsing the arguments shouldn't fail at this stage"); // Arguments were parsed previously, so they should pass parsing here

    let db = simple_db.upcast();

    let args = func
        .attributes(db)
        .find_attr(db, InternalCollector::ATTR_NAME)
        .unwrap()
        .arguments(db);

    (simple_db, args)
}
