//! Stellar 过程宏：数据库后端能力注入。
//!
//! sqlx 的多后端泛型路径存在只能按具体后端满足的 bound（`&mut Connection: Executor`、
//! `Arguments: IntoArguments`、`ColumnIndex`、值类型 Encode/Decode/Type 等），
//! 且 Rust 的 bound 不跨签名隐式传播——每个泛型上下文（impl 块 / 函数）都必须显式携带。
//!
//! 本 crate 提供两个属性宏把 bound 清单压缩成单行声明，清单在此处**单一维护**：
//! - `#[app_impl]`：用于持有 `DB` 泛型的 impl 块（services），为其全部方法注入能力
//! - `#[app_db]`：用于泛型 handler / 中间件函数，追加 `<DB: AppDb>` 泛型参数与能力 where

use proc_macro::TokenStream;
use quote::quote;

/// 数据库后端能力 bound 清单（单一事实源）。
///
/// 注意 Option<T> 的 Encode 需要直接约束（sqlx 的 blanket 要求 `T: 'q`，
/// 泛型上下文中无法从 `String: Encode` 间接推导）；Option 的 Type/Decode
/// 与 Decode-for-Option 均有无条件 blanket，无需重复。
/// 强枚举（ClusterType/DeploymentMode）的 sqlx::Type derive 只生成 per-backend
/// impl，泛型 bound 需要显式声明。
const DB_BOUNDS: &[&str] = &[
    "for<'c> &'c mut <DB as ::sqlx::Database>::Connection: ::sqlx::Executor<'c, Database = DB>",
    "for<'q> <DB as ::sqlx::database::HasArguments<'q>>::Arguments: \
     ::sqlx::IntoArguments<'q, DB> + ::std::default::Default",
    "::std::primitive::usize: ::sqlx::ColumnIndex<<DB as ::sqlx::Database>::Row>",
    "for<'a> &'a ::std::primitive::str: ::sqlx::ColumnIndex<<DB as ::sqlx::Database>::Row>",
    "for<'q> i64: ::sqlx::Encode<'q, DB> + ::sqlx::Decode<'q, DB> + ::sqlx::Type<DB>",
    "for<'q> i32: ::sqlx::Encode<'q, DB> + ::sqlx::Decode<'q, DB> + ::sqlx::Type<DB>",
    "for<'q> f64: ::sqlx::Encode<'q, DB> + ::sqlx::Decode<'q, DB> + ::sqlx::Type<DB>",
    "for<'q> bool: ::sqlx::Encode<'q, DB> + ::sqlx::Decode<'q, DB> + ::sqlx::Type<DB>",
    "for<'q> ::std::string::String: \
     ::sqlx::Encode<'q, DB> + ::sqlx::Decode<'q, DB> + ::sqlx::Type<DB>",
    "for<'q> &'q ::std::primitive::str: ::sqlx::Encode<'q, DB> + ::sqlx::Type<DB>",
    "for<'q> ::chrono::DateTime<::chrono::Utc>: \
     ::sqlx::Encode<'q, DB> + ::sqlx::Decode<'q, DB> + ::sqlx::Type<DB>",
    "for<'q> ::chrono::NaiveDateTime: \
     ::sqlx::Encode<'q, DB> + ::sqlx::Decode<'q, DB> + ::sqlx::Type<DB>",
    "for<'q> ::chrono::NaiveDate: \
     ::sqlx::Encode<'q, DB> + ::sqlx::Decode<'q, DB> + ::sqlx::Type<DB>",
    "for<'q> ::std::option::Option<&'q ::std::primitive::str>: ::sqlx::Encode<'q, DB>",
    "for<'q> ::std::option::Option<::std::string::String>: ::sqlx::Encode<'q, DB>",
    "for<'q> ::std::option::Option<i64>: ::sqlx::Encode<'q, DB>",
    "for<'q> ::std::option::Option<i32>: ::sqlx::Encode<'q, DB>",
    "for<'q> ::std::option::Option<f64>: ::sqlx::Encode<'q, DB>",
    "for<'q> ::std::option::Option<bool>: ::sqlx::Encode<'q, DB>",
    "for<'q> ::std::option::Option<::chrono::DateTime<::chrono::Utc>>: ::sqlx::Encode<'q, DB>",
    "for<'q> ::std::option::Option<::chrono::NaiveDateTime>: ::sqlx::Encode<'q, DB>",
    "for<'q> ::std::option::Option<::chrono::NaiveDate>: ::sqlx::Encode<'q, DB>",
    "for<'q> crate::models::cluster::ClusterType: \
     ::sqlx::Type<DB> + ::sqlx::Decode<'q, DB> + ::sqlx::Encode<'q, DB>",
    "for<'q> crate::models::cluster::DeploymentMode: \
     ::sqlx::Type<DB> + ::sqlx::Decode<'q, DB> + ::sqlx::Encode<'q, DB>",
];

fn inject_db_generic(generics: &mut syn::Generics) {
    let has_db = generics.params.iter().any(|p| {
        matches!(p, syn::GenericParam::Type(t) if t.ident == "DB")
    });
    if !has_db {
        generics.params.push(syn::parse_quote!(DB: crate::db::AppDb));
    }
}

fn append_bounds(generics: &mut syn::Generics) {
    if generics.where_clause.is_none() {
        generics.where_clause = Some(syn::parse_quote!(where));
    }
    let where_clause = generics.where_clause.as_mut().unwrap();

    for bound_src in DB_BOUNDS {
        let predicate: syn::WherePredicate = syn::parse_str(bound_src)
            .unwrap_or_else(|e| panic!("内部错误：bound 语法无效 '{bound_src}': {e}"));
        where_clause.predicates.push(predicate);
    }
}

/// 为持有 `DB` 泛型参数的 impl 块注入完整能力 where 子句。
///
/// ```ignore
/// #[stellar_macros::app_impl]
/// impl<DB: AppDb> AuthService<DB> { ... }
/// ```
#[proc_macro_attribute]
pub fn app_impl(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut impl_item: syn::ItemImpl = syn::parse(item)
        .expect("#[app_impl] 只能作用于 impl 块");

    append_bounds(&mut impl_item.generics);

    quote! { #impl_item }.into()
}

/// 为泛型 handler / 函数注入 `DB: AppDb` 泛型参数与完整能力 where 子句。
///
/// ```ignore
/// #[stellar_macros::app_db]
/// pub async fn list_clusters(
///     State(state): State<Arc<AppState<DB>>>,
/// ) -> ApiResult<Json<...>> { ... }
/// ```
#[proc_macro_attribute]
pub fn app_db(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut fn_item: syn::ItemFn = syn::parse(item)
        .expect("#[app_db] 只能作用于函数");

    inject_db_generic(&mut fn_item.sig.generics);
    append_bounds(&mut fn_item.sig.generics);

    quote! { #fn_item }.into()
}
