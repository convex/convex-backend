use std::{
    fmt,
    str::FromStr,
};

use super::module_path::{
    CanonicalizedModulePath,
    ModulePath,
};
use crate::function_name::FunctionName;

/// User-specified path to a function, consisting of a module path and an
/// optional function name, separated by a colon. If a function name isn't
/// provided, the UDF loader uses the default export from the module.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct UdfPath {
    module: ModulePath,
    function: Option<FunctionName>,
}

impl UdfPath {
    /// Is the named UDF a system UDF?
    pub fn is_system(&self) -> bool {
        self.module.is_system()
    }

    /// What is the module path for this UDF?
    pub fn module(&self) -> &ModulePath {
        &self.module
    }

    /// What is the function name for this UDF?
    pub fn function_name(&self) -> Option<&FunctionName> {
        self.function.as_ref()
    }

    pub fn assume_canonicalized(self) -> anyhow::Result<CanonicalizedUdfPath> {
        let module = self.module.assume_canonicalized()?;
        let function = self
            .function
            .ok_or_else(|| anyhow::anyhow!("Missing explicit ':default' function"))?;
        Ok(CanonicalizedUdfPath { module, function })
    }

    pub fn canonicalize(self) -> CanonicalizedUdfPath {
        let module = self.module.canonicalize();
        let function = self.function.unwrap_or_else(FunctionName::default_export);
        CanonicalizedUdfPath { module, function }
    }
}

impl FromStr for UdfPath {
    type Err = anyhow::Error;

    fn from_str(p: &str) -> Result<Self, Self::Err> {
        let (module, function) = match p.rsplit_once(':') {
            Some((module, function)) => (module.parse()?, Some(function.parse()?)),
            None => (p.parse()?, None),
        };
        Ok(Self { module, function })
    }
}

impl From<UdfPath> for String {
    fn from(p: UdfPath) -> Self {
        if let Some(ref function) = p.function {
            format!("{}:{}", p.module.as_str(), function)
        } else {
            p.module.as_str().to_string()
        }
    }
}

impl fmt::Display for UdfPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(ref function) = self.function {
            write!(f, "{}:{}", self.module.as_str(), function)
        } else {
            write!(f, "{}", self.module.as_str())
        }
    }
}

impl From<CanonicalizedUdfPath> for UdfPath {
    fn from(p: CanonicalizedUdfPath) -> Self {
        Self {
            module: p.module.into(),
            function: Some(p.function),
        }
    }
}

/// There are potentially multiple `UdfPath`s that address a single function, so
/// we define a notion of a "canonical" path that's in one-to-one correspondence
/// with functions the user can invoke. See the comment in `Isolate::run` for
/// more details.
#[derive(Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct CanonicalizedUdfPath {
    module: CanonicalizedModulePath,
    function: FunctionName,
}

impl CanonicalizedUdfPath {
    pub fn new(module: CanonicalizedModulePath, function: FunctionName) -> Self {
        Self { module, function }
    }

    pub fn is_system(&self) -> bool {
        self.module.is_system()
    }

    pub fn module(&self) -> &CanonicalizedModulePath {
        &self.module
    }

    pub fn function_name(&self) -> &FunctionName {
        &self.function
    }

    pub fn strip(self) -> UdfPath {
        let function = if self.function.is_default_export() {
            None
        } else {
            Some(self.function)
        };
        UdfPath {
            module: self.module.strip(),
            function,
        }
    }
}

impl fmt::Debug for CanonicalizedUdfPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}", self.module.as_str(), self.function)
    }
}

impl fmt::Display for CanonicalizedUdfPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}", self.module.as_str(), self.function)
    }
}

impl FromStr for CanonicalizedUdfPath {
    type Err = anyhow::Error;

    fn from_str(p: &str) -> Result<Self, Self::Err> {
        let path: UdfPath = p.parse()?;
        Ok(path.canonicalize())
    }
}

impl From<CanonicalizedUdfPath> for String {
    fn from(p: CanonicalizedUdfPath) -> Self {
        format!("{}:{}", p.module.as_str(), p.function)
    }
}
