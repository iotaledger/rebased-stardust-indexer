// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use clap::Args;

#[derive(Args, Debug, Clone)]
pub struct RestApiConfig {
    #[arg(long, default_value = "0.0.0.0")]
    #[arg(env = "REST_API_BIND_ADDRESS")]
    pub bind_address: String,
    #[arg(long, default_value = "3000")]
    #[arg(env = "REST_API_BIND_PORT")]
    pub bind_port: u16,
}

impl RestApiConfig {
    pub fn socket_addr(&self) -> String {
        format!("{}:{}", self.bind_address, self.bind_port)
    }
}

impl Default for RestApiConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(),
            bind_port: 3000,
        }
    }
}
