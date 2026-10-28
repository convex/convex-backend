use std::str::FromStr;

use sync_types::{
    path::PathComponent,
    CanonicalizedUdfPath,
};
use value::{
    identifier::Identifier,
    DeveloperDocumentId,
};

use super::ComponentName;

/// `References` are relative paths to `Resources` that start at some
/// component.
#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub enum Reference {
    /// Reference originating from `component.args` at component definition time
    /// or `ctx.component.args` at runtime.
    ///
    /// Definition time:
    /// ```ts
    /// const component = new ComponentDefinition({ maxLength: v.number() });
    /// const { maxLength } = component.args;
    /// // => Reference::ComponentArgument { attributes: vec!["maxLength"]}
    /// ```
    ///
    /// Runtime:
    /// ```ts
    /// import { query } from "./_generated/server";
    ///
    /// export const f = query(async (ctx, args) => {
    ///   console.log(ctx.component.args.maxLength);
    /// })
    /// ```
    ComponentArgument {
        attributes: Vec<Identifier>,
    },

    /// Reference originating from the `api` object, either at definition time
    /// or runtime.
    ///
    /// Definition time:
    /// ```ts
    /// import { api } from "./_generated/server";
    ///
    /// const f = api.foo.bar;
    /// // => Reference::Function("foo:bar")
    /// ```
    ///
    /// Runtime:
    /// ```ts
    /// import { api, mutation } from "./_generated/server";
    ///
    /// export const f = mutation({
    ///   handler: async (ctx, args) => {
    ///     await ctx.runAfter(0, api.foo.bar);
    ///   },
    /// });
    /// ```
    Function(CanonicalizedUdfPath),

    /// Reference originating from `component.childComponents` at definition
    /// time or the generated component string builders in
    /// `_generated/server` at runtime.
    ///
    /// Definition time:
    /// ```ts
    /// const component = new ComponentDefinition();
    /// const wl = component.use(waitlist);
    /// // => Reference::ChildComponent { component: "waitlist", attributes: vec![]}
    ///
    /// const f = wl.foo.bar;
    /// // => Reference::ChildComponent { component: "waitlist", attributes: vec!["foo", "bar"]}
    /// ```
    ChildComponent {
        component: ComponentName,
        attributes: Vec<PathComponent>,
    },

    CurrentSystemUdfInComponent {
        component_id: DeveloperDocumentId,
    },
}

impl Reference {
    pub fn evaluation_time_debug_str(&self) -> String {
        match self {
            Reference::ComponentArgument { attributes } => {
                let mut s = "ctx.component.args".to_string();
                for attr in attributes {
                    s.push('.');
                    s.push_str(&attr[..]);
                }
                s
            },
            Reference::Function(p) => {
                let mut s = "api".to_string();
                for component in p.module().clone().strip().as_path().components() {
                    s.push('.');
                    s.push_str(&component.as_os_str().to_string_lossy());
                }
                s.push('.');
                s.push_str(p.function_name());
                s
            },
            Reference::ChildComponent {
                component,
                attributes,
            } => {
                let mut s = component[..].to_string();
                for attr in attributes {
                    s.push('.');
                    s.push_str(&attr[..]);
                }
                s
            },
            Reference::CurrentSystemUdfInComponent { component_id } => {
                format!("_system.{component_id}")
            },
        }
    }
}

impl FromStr for Reference {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut path_components = s.split('/');
        anyhow::ensure!(
            path_components.next() == Some("_reference"),
            "Invalid reference: {s}"
        );
        let result = match path_components.next() {
            Some("componentArgument") => {
                let attributes = path_components
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                Reference::ComponentArgument { attributes }
            },
            Some("function") => {
                let remainder = path_components
                    .remainder()
                    .ok_or_else(|| anyhow::anyhow!("Invalid reference {s}"))?;
                let path = remainder.parse()?;
                Reference::Function(path)
            },
            Some("childComponent") => {
                let component = path_components
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Invalid reference {s}"))?
                    .parse()?;
                let attributes = path_components
                    .map(|s| s.parse())
                    .collect::<Result<_, _>>()?;
                Reference::ChildComponent {
                    component,
                    attributes,
                }
            },
            Some("currentSystemUdfInComponent") => {
                let component_id = path_components
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Invalid reference {s}"))?
                    .parse()?;
                Reference::CurrentSystemUdfInComponent { component_id }
            },
            Some(_) | None => anyhow::bail!("Invalid reference {s}"),
        };
        Ok(result)
    }
}

impl From<Reference> for String {
    fn from(value: Reference) -> Self {
        let mut s = "_reference".to_string();
        match value {
            Reference::ComponentArgument { attributes } => {
                s.push_str("/componentArgument");
                for attribute in attributes {
                    s.push('/');
                    s.push_str(&attribute);
                }
            },
            Reference::Function(path) => {
                s.push_str("/function");
                s.push('/');
                s.push_str(&path.to_string());
            },
            Reference::ChildComponent {
                component,
                attributes,
            } => {
                s.push_str("/childComponent");

                s.push('/');
                s.push_str(&component);

                for attribute in attributes {
                    s.push('/');
                    s.push_str(&attribute);
                }
            },
            Reference::CurrentSystemUdfInComponent { component_id } => {
                s.push_str("/currentSystemUdfInComponent");

                s.push('/');
                s.push_str(&component_id.to_string());
            },
        }
        s
    }
}
