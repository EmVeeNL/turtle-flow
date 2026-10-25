//! Ordered two-provider routing. The composition root supplies discovery status; no IO occurs
//! while choosing a run, so the selection and its audit data are deterministic.

use std::collections::HashMap;

use serde_json::json;
use turtle_domain::Role;

use crate::ProviderCapabilities;
use crate::runner::{
    AgentError, AgentOutput, AgentRequest, AgentRunner, Independence, RouteContext, RouteSelection,
};

#[derive(Debug)]
pub struct ProviderSlot<R> {
    pub runner: R,
    pub name: String,
    pub model: String,
    pub unavailable_reason: Option<String>,
}

impl<R: AgentRunner> ProviderSlot<R> {
    #[must_use]
    pub fn new(runner: R, name: impl Into<String>, unavailable_reason: Option<String>) -> Self {
        let model = runner.model().to_owned();
        Self {
            runner,
            name: name.into(),
            model,
            unavailable_reason,
        }
    }

    #[must_use]
    pub fn from_capabilities(runner: R, capabilities: &ProviderCapabilities) -> Self {
        Self::new(
            runner,
            capabilities.info.provider.as_str(),
            capabilities.exclusion_reason(),
        )
    }
}

#[derive(Debug)]
pub struct RoutingRunner<A: AgentRunner, B: AgentRunner> {
    first: ProviderSlot<A>,
    second: ProviderSlot<B>,
    order: Vec<String>,
    role_defaults: HashMap<Role, String>,
}

impl<A: AgentRunner, B: AgentRunner> RoutingRunner<A, B> {
    #[must_use]
    pub fn new(first: ProviderSlot<A>, second: ProviderSlot<B>) -> Self {
        let order = vec![first.name.clone(), second.name.clone()];
        Self {
            first,
            second,
            order,
            role_defaults: HashMap::new(),
        }
    }

    #[must_use]
    pub fn with_role_default(mut self, role: Role, provider: impl Into<String>) -> Self {
        self.role_defaults.insert(role, provider.into());
        self
    }

    fn slot(&self, name: &str) -> Option<(&str, &str, Option<&str>)> {
        if name == self.first.name {
            Some((
                &self.first.name,
                &self.first.model,
                self.first.unavailable_reason.as_deref(),
            ))
        } else if name == self.second.name {
            Some((
                &self.second.name,
                &self.second.model,
                self.second.unavailable_reason.as_deref(),
            ))
        } else {
            None
        }
    }
}

impl<A: AgentRunner, B: AgentRunner> AgentRunner for RoutingRunner<A, B> {
    fn name(&self) -> &str {
        &self.first.name
    }
    fn model(&self) -> &str {
        &self.first.model
    }

    fn select(&self, context: &RouteContext) -> Result<RouteSelection, AgentError> {
        let override_name = self.role_defaults.get(&context.role);
        if let Some(name) = override_name
            && self.slot(name).is_none()
        {
            return Err(AgentError::Fatal(format!(
                "unknown provider `{name}` in {:?} override",
                context.role
            )));
        }
        let mut order = self.order.clone();
        if let Some(name) = override_name {
            order.retain(|candidate| candidate != name);
            order.insert(0, name.clone());
        }
        if let Some(name) = &context.avoid_provider
            && order.contains(name)
        {
            order.retain(|candidate| candidate != name);
            order.push(name.clone());
        }
        let independent = matches!(context.role, Role::Reviewer | Role::Verifier)
            && context.independence != Independence::Off
            && context.implementer_provider.is_some();
        let viable_alternative = independent
            && order.iter().any(|name| {
                self.slot(name)
                    .is_some_and(|(_, _, reason)| reason.is_none())
                    && context.implementer_provider.as_deref() != Some(name)
            });
        let candidates: Vec<_> = order.iter().filter_map(|name| {
            self.slot(name).map(|(provider, model, reason)| {
                let independence_excluded = independent && (viable_alternative || context.independence == Independence::Required)
                    && context.implementer_provider.as_deref() == Some(provider);
                let limited = context.limited_providers.iter().any(|p| p == provider);
                let exclusion = reason.map(str::to_owned)
                    .or_else(|| limited.then(|| "provider currently rate limited".into()))
                    .or_else(|| independence_excluded.then(|| "same provider as implementer".into()));
                json!({"provider": provider, "model": model, "eligible": reason.is_none() && !limited && !independence_excluded,
                    "reason": exclusion})
            })
        }).collect();
        let selected = candidates
            .iter()
            .find(|candidate| candidate["eligible"] == true);
        let Some(selected) = selected else {
            let why = if independent && context.independence == Independence::Required {
                "independent review requires another available provider"
            } else {
                "no configured provider is installed and logged in"
            };
            return Err(AgentError::Fatal(why.into()));
        };
        let provider = selected["provider"]
            .as_str()
            .expect("candidate provider")
            .to_owned();
        let model = selected["model"]
            .as_str()
            .expect("candidate model")
            .to_owned();
        let fallback = independent && !viable_alternative;
        let reason = if context.avoid_provider.as_deref() == Some(provider.as_str()) {
            format!(
                "no alternative provider available for soft quota preference; selected {provider}"
            )
        } else if context.avoid_provider.is_some() {
            format!(
                "preferred another provider because {} is near quota",
                context.avoid_provider.as_deref().unwrap_or("provider")
            )
        } else if fallback {
            "independent provider unavailable; preferred review fell back to the implementer provider".into()
        } else if independent {
            "independent provider selected".into()
        } else if override_name.is_some_and(|name| name == &provider) {
            "per-role default selected".into()
        } else {
            "first available provider in fallback order".into()
        };
        Ok(RouteSelection {
            provider: provider.clone(),
            model,
            candidates: json!(candidates),
            reason,
            override_used: override_name.is_some_and(|name| name == &provider),
        })
    }

    async fn run(&self, req: &AgentRequest) -> Result<AgentOutput, AgentError> {
        let route = self.select(&RouteContext {
            role: req.role,
            independence: Independence::Off,
            implementer_provider: None,
            limited_providers: Vec::new(),
            avoid_provider: None,
        })?;
        self.run_selected(req, &route.provider).await
    }

    async fn run_selected(
        &self,
        req: &AgentRequest,
        provider: &str,
    ) -> Result<AgentOutput, AgentError> {
        if provider == self.first.name {
            self.first.runner.run(req).await
        } else if provider == self.second.name {
            self.second.runner.run(req).await
        } else {
            Err(AgentError::Fatal(format!(
                "selected provider `{provider}` is not configured"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FakeRunner;

    fn router(second_unavailable: Option<&str>) -> RoutingRunner<FakeRunner, FakeRunner> {
        RoutingRunner::new(
            ProviderSlot::new(FakeRunner::new(), "claude-code", None),
            ProviderSlot::new(
                FakeRunner::new(),
                "codex",
                second_unavailable.map(str::to_owned),
            ),
        )
    }
    fn review(independence: Independence) -> RouteContext {
        RouteContext {
            role: Role::Reviewer,
            independence,
            implementer_provider: Some("claude-code".into()),
            limited_providers: Vec::new(),
            avoid_provider: None,
        }
    }

    #[test]
    fn review_independence_and_fallback_are_explicit() {
        assert_eq!(
            router(None)
                .select(&review(Independence::Preferred))
                .expect("route")
                .provider,
            "codex"
        );
        let fallback = router(Some("not logged in"))
            .select(&review(Independence::Preferred))
            .expect("fallback");
        assert_eq!(fallback.provider, "claude-code");
        assert!(fallback.reason.contains("fell back"));
        assert_eq!(fallback.candidates[1]["reason"], "not logged in");
        let error = router(Some("not logged in"))
            .select(&review(Independence::Required))
            .expect_err("required independent provider");
        assert!(
            error
                .to_string()
                .contains("requires another available provider")
        );
        assert_eq!(
            router(None)
                .select(&review(Independence::Off))
                .expect("off")
                .provider,
            "claude-code"
        );
    }

    #[test]
    fn overrides_and_unavailable_primary_follow_order() {
        let overridden = router(None).with_role_default(Role::Planner, "codex");
        let choice = overridden
            .select(&RouteContext {
                role: Role::Planner,
                independence: Independence::Off,
                implementer_provider: None,
                limited_providers: Vec::new(),
                avoid_provider: None,
            })
            .expect("route");
        assert_eq!(choice.provider, "codex");
        assert!(choice.override_used);
        let choice = RoutingRunner::new(
            ProviderSlot::new(
                FakeRunner::new(),
                "claude-code",
                Some("not installed".into()),
            ),
            ProviderSlot::new(FakeRunner::new(), "codex", None),
        )
        .select(&RouteContext {
            role: Role::Planner,
            independence: Independence::Off,
            implementer_provider: None,
            limited_providers: Vec::new(),
            avoid_provider: None,
        })
        .expect("fallback");
        assert_eq!(choice.provider, "codex");
        assert_eq!(choice.candidates[0]["reason"], "not installed");
    }

    #[test]
    fn rate_limits_are_excluded_and_soft_quota_preferences_are_audited() {
        let context = RouteContext {
            role: Role::Implementer,
            independence: Independence::Off,
            implementer_provider: None,
            limited_providers: vec!["claude-code".into()],
            avoid_provider: None,
        };
        let route = router(None).select(&context).expect("codex fallback");
        assert_eq!(route.provider, "codex");
        assert_eq!(
            route.candidates[0]["reason"],
            "provider currently rate limited"
        );

        let preferred = router(None)
            .select(&RouteContext {
                limited_providers: Vec::new(),
                avoid_provider: Some("claude-code".into()),
                ..context
            })
            .expect("soft preference");
        assert_eq!(preferred.provider, "codex");
        assert!(preferred.reason.contains("near quota"));

        let no_alternative = RoutingRunner::new(
            ProviderSlot::new(FakeRunner::new(), "claude-code", None),
            ProviderSlot::new(FakeRunner::new(), "codex", Some("not installed".into())),
        )
        .select(&RouteContext {
            role: Role::Implementer,
            independence: Independence::Off,
            implementer_provider: None,
            limited_providers: Vec::new(),
            avoid_provider: Some("claude-code".into()),
        })
        .expect("soft guard does not block");
        assert_eq!(no_alternative.provider, "claude-code");
        assert!(no_alternative.reason.contains("no alternative"));
    }
}
