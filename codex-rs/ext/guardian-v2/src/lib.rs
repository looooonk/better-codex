use codex_extension_api::ExtensionRegistryBuilder;

mod evidence;
mod request;
mod sampler;
mod transcript;

pub use evidence::GuardianEvidenceEntry;
pub use request::GuardianReviewAction;
pub use request::GuardianReviewError;
pub use request::GuardianReviewImage;
pub use request::GuardianReviewRequest;
pub use sampler::LunaSampler;
pub use sampler::LunaSamplerConfig;
pub use sampler::LunaSamplerError;

/// Installs the Guardian V2 extension without registering contributors yet.
pub fn install<C: Sync>(_registry: &mut ExtensionRegistryBuilder<C>) {}
