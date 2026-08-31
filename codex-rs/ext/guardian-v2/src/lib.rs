use codex_extension_api::ExtensionRegistryBuilder;

mod sampler;

pub use sampler::LunaSampler;
pub use sampler::LunaSamplerConfig;
pub use sampler::LunaSamplerError;

/// Installs the Guardian V2 extension without registering contributors yet.
pub fn install<C: Sync>(_registry: &mut ExtensionRegistryBuilder<C>) {}
