//! Entry point of all test cases

//--------------------------------------------------------------------
// ATTRIBUTES TO GUARANTEE CODE QUALITY. DO NOT MODIFY.
// Warning on future incompatible features
#![warn(future_incompatible)]
// Linting rules, enabled when running Cargo with `--features linting`
#![cfg_attr(feature = "linting", warn(missing_docs))]
#![cfg_attr(feature = "linting", warn(clippy::missing_docs_in_private_items))]
#![cfg_attr(feature = "linting", warn(unused))]
#![cfg_attr(feature = "linting", warn(nonstandard_style))]
#![cfg_attr(feature = "linting", warn(clippy::perf))]
#![cfg_attr(feature = "linting", warn(clippy::style))]
#![cfg_attr(feature = "linting", warn(clippy::complexity))]
#![cfg_attr(feature = "linting", warn(clippy::suspicious))]
#![cfg_attr(feature = "linting", warn(clippy::doc_markdown))]
#![cfg_attr(feature = "linting", warn(rustdoc::broken_intra_doc_links))]
#![cfg_attr(feature = "linting", warn(rustdoc::bare_urls))]
//---------------------------------------------------------------------

mod revm_test;
