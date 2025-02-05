use cairo_lang_macro::{quote, TextSpan, Token, TokenStream, TokenTree};

pub trait CairoExpression {
    fn as_cairo_expression(&self) -> TokenStream;
}

impl<T> CairoExpression for Option<T>
where
    T: CairoExpression,
{
    fn as_cairo_expression(&self) -> TokenStream {
        if let Some(v) = self {
            let v = v.as_cairo_expression();
            quote!(Option::Some( #v ))
        } else {
            quote!(Option::None)
        }
    }
}

impl<T> CairoExpression for Vec<T>
where
    T: CairoExpression,
{
    fn as_cairo_expression(&self) -> TokenStream {
        let mut result = TokenStream::new(vec![TokenTree::Ident(Token::new(
            "array![",
            TextSpan::call_site(),
        ))]);

        for e in self {
            result.extend(e.as_cairo_expression().into_iter());

            result.push_token(TokenTree::Ident(Token::new(",", TextSpan::call_site())));
        }

        result.push_token(TokenTree::Ident(Token::new("]", TextSpan::call_site())));
        result
    }
}
