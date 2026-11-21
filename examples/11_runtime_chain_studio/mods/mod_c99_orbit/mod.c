/* ============================================================================
 * mod.c — Pure ISO C99 Guest Mod: `mod_c99_orbit` (`C99PlanetaryOrbit`, ABI 0.3.0)
 *
 * Zero Rust dependencies! Compiles with strict `-std=c99 -Wall -Wextra -Werror`.
 * Sweeps 3D actors into tilted multi-ring Keplerian planetary orbits using
 * contiguous zero-serialization SoA columns + singleton `studio::SimClock`.
 * ============================================================================ */

#include "bamabi.h"
#include <math.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef struct StudioTransform3D {
    float px, py, pz;
    float rx, ry, rz;
    float sx, sy, sz;
    uint32_t shape;
} StudioTransform3D;

typedef struct StudioMotion3D {
    float vx, vy, vz;
    float avx, avy, avz;
    float speed;
    float phase;
} StudioMotion3D;

typedef struct StudioVisualStyle {
    float r, g, b, a;
    float emissive;
    float hue;
    float energy;
    float pulse;
} StudioVisualStyle;

typedef struct StudioActorMeta {
    uint32_t index;
    uint32_t group;
    float home_x, home_y, home_z;
    uint32_t flags;
} StudioActorMeta;

typedef struct StudioSimClock {
    uint64_t tick;
    float dt;
    float elapsed;
    float speed_scale;
    float amplitude;
    uint32_t active_chain_len;
    uint32_t actor_count;
} StudioSimClock;

typedef struct C99OrbitState {
    BamHostApi api;
    uint64_t local_steps;
} C99OrbitState;

static const BamManifest MANIFEST = {
    .struct_size = (uint32_t)sizeof(BamManifest),
    .abi_version = BAM_ABI_VERSION,
    .name = "mod_c99_orbit",
    .version = "0.3.0",
    .flags = 0u,
    .reserved0 = 0u,
    .save_state = NULL,
    .restore_state = NULL
};

static BamResult c99_planetary_orbit_system(BamSystemCtx* sys_ctx, void* user_data) {
    C99OrbitState* st = (C99OrbitState*)user_data;
    BamIter* iter = NULL;
    BamColumnInfo xf_info, mot_info, vis_info, meta_info;
    const StudioSimClock* clock_ptr = NULL;
    float dt = 1.0f / 60.0f;
    float t = 0.0f;
    float amp = 1.0f;
    size_t count = 0u;
    BamResult res;

    if (st == NULL || sys_ctx == NULL) {
        return BAM_RESULT_NULL_POINTER;
    }

    res = st->api.sys_query(sys_ctx, 0u, &iter);
    if (res != BAM_RESULT_OK) {
        return res;
    }

    if (st->api.iter_column_info(iter, 0u, &xf_info) != BAM_RESULT_OK ||
        st->api.iter_column_info(iter, 1u, &mot_info) != BAM_RESULT_OK ||
        st->api.iter_column_info(iter, 2u, &vis_info) != BAM_RESULT_OK ||
        st->api.iter_column_info(iter, 3u, &meta_info) != BAM_RESULT_OK) {
        return BAM_RESULT_TYPE_MISMATCH;
    }

    if (xf_info.size != sizeof(StudioTransform3D) ||
        mot_info.size != sizeof(StudioMotion3D) ||
        vis_info.size != sizeof(StudioVisualStyle) ||
        meta_info.size != sizeof(StudioActorMeta)) {
        return BAM_RESULT_TYPE_MISMATCH;
    }

    clock_ptr = (const StudioSimClock*)st->api.res_ptr(iter, 4u);
    if (clock_ptr != NULL) {
        dt = clock_ptr->dt * clock_ptr->speed_scale;
        t = clock_ptr->elapsed * clock_ptr->speed_scale;
        amp = clock_ptr->amplitude;
    } else {
        st->local_steps += 1u;
        t = (float)st->local_steps * dt;
    }
    if (dt < 0.0f) dt = 0.0f;
    if (dt > 0.05f) dt = 0.05f;

    while (st->api.iter_next_chunk(iter, &count) != BAM_FALSE && count > 0u) {
        StudioTransform3D* xfs  = (StudioTransform3D*)st->api.iter_column_mutable(iter, 0u);
        StudioMotion3D* mots    = (StudioMotion3D*)st->api.iter_column_mutable(iter, 1u);
        StudioVisualStyle* viss = (StudioVisualStyle*)st->api.iter_column_mutable(iter, 2u);
        const StudioActorMeta* metas =
            (const StudioActorMeta*)st->api.iter_column_readonly(iter, 3u);
        size_t i;

        if (!xfs || !mots || !viss || !metas) {
            return BAM_RESULT_NULL_POINTER;
        }

        for (i = 0u; i < count; ++i) {
            uint32_t idx;
            uint32_t ring;
            float ring_r;
            float omega;
            float angle;
            float tilt;
            float tx, ty, tz;
            float dx, dy, dz;

            if ((metas[i].flags & 1u) != 0u) {
                continue;
            }

            idx = metas[i].index;
            ring = idx % 4u;
            ring_r = (2.2f + (float)ring * 2.15f) * (0.75f + 0.25f * amp);
            omega = (1.55f / (0.65f + (float)ring * 0.45f)) * ((ring % 2u == 0u) ? 1.0f : -1.0f);
            angle = (float)idx * 2.3999632f + t * omega;
            tilt = (0.28f + 0.14f * (float)ring) * amp;

            tx = ring_r * cosf(angle);
            tz = ring_r * sinf(angle);
            ty = 2.6f + sinf(angle + (float)ring * 1.1f) * ring_r * sinf(tilt);
            if (ty < 0.45f) ty = 0.45f;

            dx = tx - xfs[i].px;
            dy = ty - xfs[i].py;
            dz = tz - xfs[i].pz;

            mots[i].vx = dx * 5.0f;
            mots[i].vy = dy * 5.0f;
            mots[i].vz = dz * 5.0f;

            xfs[i].px += dx * 0.18f;
            xfs[i].py += dy * 0.18f;
            xfs[i].pz += dz * 0.18f;
            xfs[i].ry = atan2f(-sinf(angle), cosf(angle));

            viss[i].energy = viss[i].energy * 0.78f + (0.45f + 0.25f * (float)(3u - ring)) * 0.45f;
            if (viss[i].energy > 2.0f) viss[i].energy = 2.0f;
        }
    }

    return BAM_RESULT_OK;
}

BAM_EXPORT const BamManifest* bamabi_manifest(void) {
    return &MANIFEST;
}

BAM_EXPORT BamResult bamabi_init(
    const BamHostApi* host_api,
    BamLoadCtx*       load_ctx,
    BamInstance**     instance_out
) {
    BamHostApi local_api;
    const BamHostCtx* host_ctx;
    C99OrbitState* st;
    BamComponent xf_h, mot_h, vis_h, meta_h, clock_h;
    BamTermDesc terms[5];
    BamQueryDesc query_desc;
    BamSystemDesc sys_desc;
    BamResult res;

    if (load_ctx == NULL || instance_out == NULL) {
        return BAM_RESULT_NULL_POINTER;
    }
    *instance_out = NULL;

    res = bam_host_api_copy_bounded(&local_api, sizeof(local_api), host_api);
    if (res != BAM_RESULT_OK) {
        return res;
    }

    if (local_api.load_host_ctx == NULL ||
        local_api.resolve_component == NULL ||
        local_api.register_system == NULL ||
        local_api.sys_query == NULL ||
        local_api.iter_next_chunk == NULL ||
        local_api.iter_column_mutable == NULL ||
        local_api.iter_column_readonly == NULL ||
        local_api.iter_column_info == NULL ||
        local_api.res_ptr == NULL) {
        return BAM_RESULT_NULL_POINTER;
    }

    st = (C99OrbitState*)calloc(1u, sizeof(C99OrbitState));
    if (st == NULL) {
        return BAM_RESULT_INTERNAL_ERROR;
    }
    st->api = local_api;

    host_ctx = st->api.load_host_ctx(load_ctx);
    if (host_ctx == NULL) {
        free(st);
        return BAM_RESULT_NULL_POINTER;
    }

    if (st->api.resolve_component(host_ctx, "studio::Transform3D", NULL, &xf_h) != BAM_RESULT_OK ||
        st->api.resolve_component(host_ctx, "studio::Motion3D", NULL, &mot_h) != BAM_RESULT_OK ||
        st->api.resolve_component(host_ctx, "studio::VisualStyle", NULL, &vis_h) != BAM_RESULT_OK ||
        st->api.resolve_component(host_ctx, "studio::ActorMeta", NULL, &meta_h) != BAM_RESULT_OK ||
        st->api.resolve_component(host_ctx, "studio::SimClock", NULL, &clock_h) != BAM_RESULT_OK) {
        free(st);
        return BAM_RESULT_INVALID_COMPONENT;
    }

    terms[0] = bam_term_desc(xf_h,    BAM_ACCESS_WRITE, 0u);
    terms[1] = bam_term_desc(mot_h,   BAM_ACCESS_WRITE, 0u);
    terms[2] = bam_term_desc(vis_h,   BAM_ACCESS_WRITE, 0u);
    terms[3] = bam_term_desc(meta_h,  BAM_ACCESS_READ, 0u);
    terms[4] = bam_term_desc(clock_h, BAM_ACCESS_RESOURCE_READ, 0u);

    memset(&query_desc, 0, sizeof(query_desc));
    query_desc.struct_size = (uint32_t)sizeof(BamQueryDesc);
    query_desc.terms       = terms;
    query_desc.term_count  = 5u;

    memset(&sys_desc, 0, sizeof(sys_desc));
    sys_desc.struct_size = (uint32_t)sizeof(BamSystemDesc);
    sys_desc.name        = "C99PlanetaryOrbit";
    sys_desc.queries     = &query_desc;
    sys_desc.query_count = 1u;
    sys_desc.callback    = c99_planetary_orbit_system;
    sys_desc.user_data   = st;
    sys_desc.schedule    = BAM_SCHEDULE_UPDATE;

    res = st->api.register_system(load_ctx, &sys_desc);
    if (res != BAM_RESULT_OK) {
        free(st);
        return res;
    }

    *instance_out = (BamInstance*)st;
    return BAM_RESULT_OK;
}

BAM_EXPORT void bamabi_shutdown(BamInstance* instance) {
    if (instance != NULL) {
        free(instance);
    }
}
