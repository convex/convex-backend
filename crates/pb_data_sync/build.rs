use std::io::Result;

use pb_build::pb_build;

fn main() -> Result<()> {
    let extra_includes = vec!["../pb/protos"];
    pb_build(vec![], extra_includes)
}
