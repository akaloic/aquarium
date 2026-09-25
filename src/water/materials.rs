use bevy::{
    mesh::MeshVertexBufferLayoutRef,
    pbr::{
        ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
        MaterialPipeline, MaterialPipelineKey,
    },
    prelude::*,
    render::render_resource::{
        AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};

use crate::tank::disable_depth_write;

// ---------------------------------------------------------------------------
// Surface seen from above
// ---------------------------------------------------------------------------

pub type WaterSurfaceMaterial = ExtendedMaterial<StandardMaterial, WaterSurfaceExt>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct WaterSurfaceExt {
    /// x: ripple strength, y: time scale, z: close-up boost.
    #[uniform(100)]
    pub ripple: Vec4,
    /// Rings from dropped food: (x, z, start time, strength).
    #[uniform(100)]
    pub drops: [Vec4; 4],
}

impl MaterialExtension for WaterSurfaceExt {
    fn fragment_shader() -> ShaderRef {
        "shaders/water_surface.wgsl".into()
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        disable_depth_write(descriptor);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Surface seen from below
// ---------------------------------------------------------------------------

#[derive(ShaderType, Clone, Copy, Debug, Default)]
pub struct UndersideParams {
    pub mirror_color: Vec4,
    pub floor_color: Vec4,
    pub sky_color: Vec4,
    pub light: Vec4,
    pub ripple: Vec4,
    pub drops: [Vec4; 4],
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct WaterUndersideMaterial {
    #[uniform(0)]
    pub params: UndersideParams,
}

impl Material for WaterUndersideMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/water_underside.wgsl".into()
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Custom materials get no culling in the prepass; this mesh must only
        // exist when seen from below, or it would hide the tank from above.
        descriptor.primitive.cull_mode = Some(Face::Back);
        Ok(())
    }
}
