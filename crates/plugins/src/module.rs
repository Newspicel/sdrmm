use std::{
    path::Path,
    sync::{Arc, Mutex, PoisonError},
};

use sdrmm_wire::plugin::{ABI_VERSION, Op, PluginManifest, Reply, export, unpack};
use serde::{Serialize, de::DeserializeOwned};
use wasmtime::{Instance, Linker, Memory, Module, Store, TypedFunc};

use crate::{
    PluginError,
    cache::ModuleCache,
    engine::{CALL_BUDGET, engine},
    kernels::{HostState, link},
    processor::check_processor,
    transmit::check_exports,
};

/// One loaded plugin: its compiled code, its manifest, and an instance kept for control calls.
pub struct PluginModule {
    pub(crate) name: String,
    module: Module,
    linker: Linker<HostState>,
    control: Mutex<Guest>,
    pub(crate) manifest: PluginManifest,
}

/// A running instance of a plugin and the exports every call needs.
pub(crate) struct Guest {
    pub(crate) store: Store<HostState>,
    pub(crate) instance: Instance,
    pub(crate) memory: Memory,
    alloc: TypedFunc<u32, u32>,
    call: TypedFunc<(i32, u32, u32), i64>,
    error: TypedFunc<(), i64>,
}

impl PluginModule {
    pub fn load(path: &Path) -> Result<Arc<Self>, PluginError> {
        let name = plugin_name(path);
        let bytes = std::fs::read(path).map_err(|error| PluginError::Load {
            plugin: name.clone(),
            error: error.to_string(),
        })?;
        Self::from_bytes(name, &bytes, None)
    }

    /// Compiles the module in `bytes`, or takes its build from `cache`, `name` being what logs
    /// and errors call it.
    pub fn from_bytes(
        name: String,
        bytes: &[u8],
        cache: Option<&ModuleCache>,
    ) -> Result<Arc<Self>, PluginError> {
        let fail = |error: String| PluginError::Load {
            plugin: name.clone(),
            error,
        };
        let engine = engine()?;
        let module = match cache {
            Some(cache) => cache.module(engine, &name, bytes),
            None => Module::new(engine, bytes).map_err(|error| error.to_string()),
        }
        .map_err(fail)?;
        let mut linker = Linker::new(engine);
        link(&mut linker)?;
        let mut control = instantiate(&name, &module, &linker).map_err(fail)?;
        let manifest = control.manifest().map_err(fail)?;
        check_abi(manifest.abi).map_err(fail)?;
        if manifest.descriptor.can_transmit {
            check_exports(&module).map_err(fail)?;
        }
        check_processor(&module, &manifest).map_err(fail)?;
        Ok(Arc::new(Self {
            name,
            module,
            linker,
            control: Mutex::new(control),
            manifest,
        }))
    }

    pub(crate) fn instantiate(&self) -> Result<Guest, String> {
        instantiate(&self.name, &self.module, &self.linker)
    }

    /// A control call on the shared instance: JSON in, JSON reply out.
    pub(crate) fn call<T: DeserializeOwned>(
        &self,
        op: Op,
        input: &impl Serialize,
    ) -> Result<T, String> {
        let mut guest = self.control.lock().unwrap_or_else(PoisonError::into_inner);
        guest.call(op, input)
    }
}

fn instantiate(name: &str, module: &Module, linker: &Linker<HostState>) -> Result<Guest, String> {
    let engine = module.engine();
    let mut store = Store::new(engine, HostState::new(name));
    store.limiter(|state| &mut state.limits);
    store.set_epoch_deadline(CALL_BUDGET);
    let instance = linker
        .instantiate(&mut store, module)
        .map_err(|error| error.to_string())?;
    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or_else(|| "exports no memory".to_owned())?;
    let abi: TypedFunc<(), u32> = typed(&instance, &mut store, export::ABI)?;
    let version = abi
        .call(&mut store, ())
        .map_err(|error| error.to_string())?;
    check_abi(version)?;
    Ok(Guest {
        alloc: typed(&instance, &mut store, export::ALLOC)?,
        call: typed(&instance, &mut store, export::CALL)?,
        error: typed(&instance, &mut store, export::ERROR)?,
        store,
        instance,
        memory,
    })
}

fn guest_span(memory: &[u8], ptr: u32, len: u32) -> Option<&[u8]> {
    let start = usize::try_from(ptr).ok()?;
    let end = start.checked_add(usize::try_from(len).ok()?)?;
    memory.get(start..end)
}

pub(crate) fn plugin_name(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn check_abi(version: u32) -> Result<(), String> {
    if version == ABI_VERSION {
        Ok(())
    } else {
        Err(format!(
            "built for plugin ABI {version}, this server speaks ABI {ABI_VERSION}"
        ))
    }
}

pub(crate) fn typed<P: wasmtime::WasmParams, R: wasmtime::WasmResults>(
    instance: &Instance,
    store: &mut Store<HostState>,
    name: &str,
) -> Result<TypedFunc<P, R>, String> {
    instance
        .get_typed_func::<P, R>(&mut *store, name)
        .map_err(|error| format!("export {name}: {error}"))
}

impl Guest {
    fn manifest(&mut self) -> Result<PluginManifest, String> {
        let manifest: TypedFunc<(), i64> =
            typed(&self.instance, &mut self.store, export::MANIFEST)?;
        self.store.set_epoch_deadline(CALL_BUDGET);
        let packed = manifest
            .call(&mut self.store, ())
            .map_err(|error| error.to_string())?;
        let bytes = self.read_packed(packed)?;
        serde_json::from_slice(&bytes).map_err(|error| format!("manifest: {error}"))
    }

    fn call<T: DeserializeOwned>(&mut self, op: Op, input: &impl Serialize) -> Result<T, String> {
        let bytes = serde_json::to_vec(input).map_err(|error| error.to_string())?;
        let (ptr, len) = self.write(&bytes)?;
        self.store.set_epoch_deadline(CALL_BUDGET);
        let packed = self
            .call
            .call(&mut self.store, (op as i32, ptr, len))
            .map_err(|error| error.to_string())?;
        let reply: Reply<T> = serde_json::from_slice(&self.read_packed(packed)?)
            .map_err(|error| format!("reply: {error}"))?;
        reply.into_result()
    }

    /// Calls `func(first, json)` with `value` as JSON in a guest buffer.
    pub(crate) fn call_json<A: wasmtime::WasmTy>(
        &mut self,
        func: &TypedFunc<(A, u32, u32), i32>,
        first: A,
        value: &impl Serialize,
    ) -> Result<i32, String> {
        let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
        let (ptr, len) = self.write(&bytes)?;
        self.store.set_epoch_deadline(CALL_BUDGET);
        func.call(&mut self.store, (first, ptr, len))
            .map_err(|error| error.to_string())
    }

    /// Copies `bytes` into a buffer the guest allocated for them.
    pub(crate) fn write(&mut self, bytes: &[u8]) -> Result<(u32, u32), String> {
        let len = u32::try_from(bytes.len()).map_err(|_| "message too large".to_owned())?;
        self.store.set_epoch_deadline(CALL_BUDGET);
        let ptr = self
            .alloc
            .call(&mut self.store, len)
            .map_err(|error| error.to_string())?;
        if len > 0 {
            self.memory
                .write(&mut self.store, ptr as usize, bytes)
                .map_err(|error| error.to_string())?;
        }
        Ok((ptr, len))
    }

    /// The bytes a packed pointer and length name, checked against the guest's memory before
    /// anything is allocated for them.
    pub(crate) fn read_packed(&self, packed: i64) -> Result<Vec<u8>, String> {
        let (ptr, len) = unpack(packed);
        guest_span(self.memory.data(&self.store), ptr, len)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| format!("reply of {len} bytes at {ptr} lies outside guest memory"))
    }

    /// The message the guest left after a failed call.
    pub(crate) fn last_error(&mut self) -> String {
        self.store.set_epoch_deadline(CALL_BUDGET);
        self.error
            .call(&mut self.store, ())
            .map_err(|error| error.to_string())
            .and_then(|packed| self.read_packed(packed))
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_else(|error| error)
    }
}

#[cfg(test)]
mod tests {
    use super::guest_span;

    #[test]
    fn a_span_past_guest_memory_is_refused() {
        let memory = [1u8, 2, 3, 4];
        assert_eq!(guest_span(&memory, 1, 2), Some([2u8, 3].as_slice()));
        assert_eq!(guest_span(&memory, 4, 0), Some([].as_slice()));
        assert_eq!(guest_span(&memory, 3, 2), None);
        assert_eq!(guest_span(&memory, u32::MAX, u32::MAX), None);
    }
}
