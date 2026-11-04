use std::{
    path::Path,
    sync::{Arc, Mutex, PoisonError},
};

use sdrmm_wire::plugin::{ABI_VERSION, Op, PluginManifest, Reply, export, unpack};
use serde::{Serialize, de::DeserializeOwned};
use wasmtime::{Instance, Linker, Memory, Module, Store, TypedFunc};

use crate::{
    PluginError,
    engine::{CALL_BUDGET, engine},
    kernels::{HostState, link},
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
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let fail = |error: String| PluginError::Load {
            plugin: name.clone(),
            error,
        };
        let engine = engine()?;
        let module = Module::from_file(engine, path).map_err(|error| fail(error.to_string()))?;
        let mut linker = Linker::new(engine);
        link(&mut linker)?;
        let mut control = instantiate(&name, &module, &linker).map_err(fail)?;
        let manifest = control.manifest().map_err(fail)?;
        if manifest.abi != ABI_VERSION {
            return Err(fail(format!(
                "speaks plugin ABI {}, this server speaks {ABI_VERSION}",
                manifest.abi
            )));
        }
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
    if version != ABI_VERSION {
        return Err(format!(
            "speaks plugin ABI {version}, this server speaks {ABI_VERSION}"
        ));
    }
    Ok(Guest {
        alloc: typed(&instance, &mut store, export::ALLOC)?,
        call: typed(&instance, &mut store, export::CALL)?,
        error: typed(&instance, &mut store, export::ERROR)?,
        store,
        instance,
        memory,
    })
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

    pub(crate) fn read_packed(&self, packed: i64) -> Result<Vec<u8>, String> {
        let (ptr, len) = unpack(packed);
        let mut bytes = vec![0u8; len as usize];
        self.memory
            .read(&self.store, ptr as usize, &mut bytes)
            .map_err(|error| error.to_string())?;
        Ok(bytes)
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
