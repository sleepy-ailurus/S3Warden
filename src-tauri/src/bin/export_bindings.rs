//! 独立导出 TypeScript 绑定，供 CI 漂移检查（IPC-04）：
//!
//! ```sh
//! cargo run -p s3warden --bin export-bindings
//! git diff --exit-code -- web/src/bindings.ts
//! ```

fn main() {
    s3warden_lib::export_bindings();
    println!("bindings exported");
}
