fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Proto files go to OUT_DIR by default; include them with tonic::include_proto!
    tonic_prost_build::compile_protos("proto/conflux.proto")?;
    Ok(())
}
