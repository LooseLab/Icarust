pub mod minknow_api {
    pub(crate) mod analysis_configuration {
        tonic::include_proto!("minknow_api.analysis_configuration");
    }
    pub(crate) mod acquisition {
        tonic::include_proto!("minknow_api.acquisition");
    }
    pub(crate) mod basecaller {
        tonic::include_proto!("minknow_api.basecaller");
    }
    pub(crate) mod data {
        tonic::include_proto!("minknow_api.data");
    }
    pub(crate) mod device {
        tonic::include_proto!("minknow_api.device");
    }
    pub(crate) mod instance {
        tonic::include_proto!("minknow_api.instance");
    }
    pub(crate) mod keystore {
        tonic::include_proto!("minknow_api.keystore");
    }
    pub(crate) mod log {
        tonic::include_proto!("minknow_api.log");
    }
    pub(crate) mod manager {
        tonic::include_proto!("minknow_api.manager");
    }
    pub(crate) mod minion_device {
        tonic::include_proto!("minknow_api.minion_device");
    }
    pub(crate) mod promethion_device {
        tonic::include_proto!("minknow_api.promethion_device");
    }
    pub(crate) mod protocol_settings {
        tonic::include_proto!("minknow_api.protocol_settings");
    }
    pub(crate) mod protocol {
        tonic::include_proto!("minknow_api.protocol");
    }
    pub(crate) mod statistics {
        tonic::include_proto!("minknow_api.statistics");
    }
}

pub mod setup_conf {
    pub fn get_channel_size() -> usize {
        3000
    }
}
