fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
         .build_client(false)
         .compile(
             &["proto/minknow_api/manager.proto",
               "proto/minknow_api/instance.proto",
               "proto/minknow_api/log.proto",
               "proto/minknow_api/device.proto",
               "proto/minknow_api/basecaller.proto",
               ],
      &["proto/"],
         )?;
    Ok(())
 }