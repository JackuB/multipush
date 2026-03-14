use crate::recipe::Recipe;
use crate::Result;

const CODEOWNERS: &str = include_str!("builtins/codeowners.yml");
const SECURITY_MD: &str = include_str!("builtins/security-md.yml");
const LICENSE: &str = include_str!("builtins/license.yml");
const EDITORCONFIG: &str = include_str!("builtins/editorconfig.yml");
const GITIGNORE: &str = include_str!("builtins/gitignore.yml");
const DEPENDABOT: &str = include_str!("builtins/dependabot.yml");

pub fn builtin_recipes() -> Result<Vec<Recipe>> {
    let yamls = [
        CODEOWNERS,
        SECURITY_MD,
        LICENSE,
        EDITORCONFIG,
        GITIGNORE,
        DEPENDABOT,
    ];
    yamls.iter().map(|y| Recipe::from_yaml(y)).collect()
}
