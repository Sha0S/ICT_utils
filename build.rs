extern crate embed_resource;

fn main() {
    // None for: "auth_manager"

    embed_resource::compile_for("ccl_interlock.rc", &["ccl_interlock"], embed_resource::NONE);
    embed_resource::compile_for("analysis.rc", &["analysis"], embed_resource::NONE);
    embed_resource::compile_for("log_reader.rc", &["log_reader"], embed_resource::NONE);
    embed_resource::compile_for("query.rc", &["query", "smt_yield"], embed_resource::NONE);

    embed_resource::compile_for(
        "traceability.rc",
        &[
            "traceability-client",
            "traceability-server",
            "aoi_uploader",
            "spi_uploader",
            "ccl5_uploader",
        ],
        embed_resource::NONE,
    );
}
