use anyhow::Context;
use sdrmm_server::OidcOptions;
use sdrmm_wire::UserRole;

/// Sign-in with an OIDC identity provider such as Pocket ID, Authelia or Keycloak.
#[derive(clap::Args, Debug, Default)]
pub(crate) struct SignInArgs {
    /// The address browsers open this server at, such as `https://sdr.example.org`.
    #[arg(long, env = "SDRMM_PUBLIC_URL")]
    public_url: Option<url::Url>,
    #[arg(long, env = "SDRMM_OIDC_ISSUER", requires_all = ["oidc_client_id", "public_url"])]
    oidc_issuer: Option<String>,
    #[arg(long, env = "SDRMM_OIDC_CLIENT_ID", requires = "oidc_issuer")]
    oidc_client_id: Option<String>,
    #[arg(
        long,
        env = "SDRMM_OIDC_CLIENT_SECRET",
        hide_env_values = true,
        requires = "oidc_issuer"
    )]
    oidc_client_secret: Option<String>,
    /// The claim that lists a user's groups.
    #[arg(long, env = "SDRMM_OIDC_GROUPS_CLAIM", default_value = "groups")]
    oidc_groups_claim: String,
    #[arg(
        long = "oidc-admin-group",
        env = "SDRMM_OIDC_ADMIN_GROUPS",
        value_delimiter = ','
    )]
    oidc_admin_groups: Vec<String>,
    #[arg(
        long = "oidc-operator-group",
        env = "SDRMM_OIDC_OPERATOR_GROUPS",
        value_delimiter = ','
    )]
    oidc_operator_groups: Vec<String>,
    #[arg(
        long = "oidc-viewer-group",
        env = "SDRMM_OIDC_VIEWER_GROUPS",
        value_delimiter = ','
    )]
    oidc_viewer_groups: Vec<String>,
    /// The role of a user in none of the groups. Without it, they are turned away.
    #[arg(long, env = "SDRMM_OIDC_DEFAULT_ROLE", value_parser = parse_role)]
    oidc_default_role: Option<UserRole>,
}

impl SignInArgs {
    pub(crate) fn options(self) -> anyhow::Result<Option<OidcOptions>> {
        let Some(issuer) = self.oidc_issuer else {
            return Ok(None);
        };
        let options = OidcOptions {
            issuer,
            client_id: self
                .oidc_client_id
                .context("--oidc-client-id is required")?,
            client_secret: self.oidc_client_secret.filter(|secret| !secret.is_empty()),
            public_url: self.public_url.context("--public-url is required")?,
            groups_claim: self.oidc_groups_claim,
            admin_groups: self.oidc_admin_groups,
            operator_groups: self.oidc_operator_groups,
            viewer_groups: self.oidc_viewer_groups,
            default_role: self.oidc_default_role,
        };
        if options.default_role.is_none()
            && options.admin_groups.is_empty()
            && options.operator_groups.is_empty()
            && options.viewer_groups.is_empty()
        {
            anyhow::bail!(
                "sign-in needs a group flag such as --oidc-admin-group, or --oidc-default-role"
            );
        }
        Ok(Some(options))
    }
}

fn parse_role(raw: &str) -> Result<UserRole, String> {
    UserRole::parse(raw).ok_or_else(|| format!("`{raw}` is not admin, operator or viewer"))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    struct Probe {
        #[command(flatten)]
        sign_in: SignInArgs,
    }

    fn parse(args: &[&str]) -> anyhow::Result<Option<OidcOptions>> {
        let mut argv = vec!["sdrmm"];
        argv.extend_from_slice(args);
        Probe::try_parse_from(argv)?.sign_in.options()
    }

    #[test]
    fn sign_in_is_off_without_an_issuer() {
        assert!(parse(&[]).expect("parse").is_none());
    }

    #[test]
    fn groups_map_to_roles() {
        let options = parse(&[
            "--oidc-issuer",
            "https://id.example.org",
            "--oidc-client-id",
            "sdrmm",
            "--public-url",
            "https://sdr.example.org",
            "--oidc-admin-group",
            "sdr-admins",
            "--oidc-operator-group",
            "radio,ops",
        ])
        .expect("parse")
        .expect("on");
        assert_eq!(
            options.role_for(&["ops".to_owned()]),
            Some(UserRole::Operator)
        );
        assert_eq!(
            options.role_for(&["sdr-admins".to_owned()]),
            Some(UserRole::Admin)
        );
        assert_eq!(options.role_for(&[]), None);
    }

    #[test]
    fn an_issuer_needs_a_client_id_a_public_url_and_a_role() {
        assert!(parse(&["--oidc-issuer", "https://id.example.org"]).is_err());
        assert!(
            parse(&[
                "--oidc-issuer",
                "https://id.example.org",
                "--oidc-client-id",
                "sdrmm",
                "--public-url",
                "https://sdr.example.org",
            ])
            .is_err()
        );
        assert!(
            parse(&[
                "--oidc-issuer",
                "https://id.example.org",
                "--oidc-client-id",
                "sdrmm",
                "--public-url",
                "https://sdr.example.org",
                "--oidc-default-role",
                "viewer",
            ])
            .is_ok()
        );
        assert!(parse(&["--oidc-default-role", "root"]).is_err());
    }
}
