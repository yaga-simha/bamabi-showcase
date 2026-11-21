// ============================================================================
// mod.cpp — Strict ISO C++17 Guest Mod: `mod_cpp_gravity` (`CppBounceGravity`, ABI 0.3.0)
//
// Zero Rust dependencies! RAII per-World instance + `try/catch (...)` exception
// containment at every `extern "C"` boundary (`spec.md` §3.4).
// Applies 3D gravity, rhythmic elastic floor bounces, and kinetic shock energy!
// ============================================================================

#include "bamabi.h"

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>

namespace {

struct StudioTransform3D {
    float px, py, pz;
    float rx, ry, rz;
    float sx, sy, sz;
    uint32_t shape;
};

struct StudioMotion3D {
    float vx, vy, vz;
    float avx, avy, avz;
    float speed;
    float phase;
};

struct StudioVisualStyle {
    float r, g, b, a;
    float emissive;
    float hue;
    float energy;
    float pulse;
};

struct StudioActorMeta {
    uint32_t index;
    uint32_t group;
    float home_x, home_y, home_z;
    uint32_t flags;
};

struct StudioSimClock {
    uint64_t tick;
    float dt;
    float elapsed;
    float speed_scale;
    float amplitude;
    uint32_t active_chain_len;
    uint32_t actor_count;
};

static_assert(sizeof(StudioTransform3D) == 40u, "StudioTransform3D must be 40 bytes");
static_assert(sizeof(StudioMotion3D) == 32u, "StudioMotion3D must be 32 bytes");
static_assert(sizeof(StudioVisualStyle) == 32u, "StudioVisualStyle must be 32 bytes");
static_assert(sizeof(StudioActorMeta) == 24u, "StudioActorMeta must be 24 bytes");
static_assert(sizeof(StudioSimClock) == 32u, "StudioSimClock must be 32 bytes");

constexpr BamManifest kManifest = {
    static_cast<uint32_t>(sizeof(BamManifest)),
    BAM_ABI_VERSION,
    "mod_cpp_gravity",
    "0.3.0",
    0u,
    0u,
    nullptr,
    nullptr
};

class CppGravityInstance {
public:
    explicit CppGravityInstance(const BamHostApi& api) noexcept : api_(api) {}
    ~CppGravityInstance() = default;

    CppGravityInstance(const CppGravityInstance&) = delete;
    CppGravityInstance& operator=(const CppGravityInstance&) = delete;

    BamResult init(BamLoadCtx* load_ctx) {
        if (!api_.load_host_ctx || !api_.resolve_component || !api_.register_system ||
            !api_.sys_query || !api_.iter_next_chunk || !api_.iter_column_mutable ||
            !api_.iter_column_readonly || !api_.iter_column_info || !api_.res_ptr) {
            return BAM_RESULT_NULL_POINTER;
        }

        const BamHostCtx* host_ctx = api_.load_host_ctx(load_ctx);
        if (!host_ctx) {
            return BAM_RESULT_NULL_POINTER;
        }

        BamComponent xf_h{}, mot_h{}, vis_h{}, meta_h{}, clock_h{};
        if (api_.resolve_component(host_ctx, "studio::Transform3D", nullptr, &xf_h) != BAM_RESULT_OK ||
            api_.resolve_component(host_ctx, "studio::Motion3D", nullptr, &mot_h) != BAM_RESULT_OK ||
            api_.resolve_component(host_ctx, "studio::VisualStyle", nullptr, &vis_h) != BAM_RESULT_OK ||
            api_.resolve_component(host_ctx, "studio::ActorMeta", nullptr, &meta_h) != BAM_RESULT_OK ||
            api_.resolve_component(host_ctx, "studio::SimClock", nullptr, &clock_h) != BAM_RESULT_OK) {
            return BAM_RESULT_INVALID_COMPONENT;
        }

        BamTermDesc terms[5] = {
            bam_term_desc(xf_h,    BAM_ACCESS_WRITE, 0u),
            bam_term_desc(mot_h,   BAM_ACCESS_WRITE, 0u),
            bam_term_desc(vis_h,   BAM_ACCESS_WRITE, 0u),
            bam_term_desc(meta_h,  BAM_ACCESS_READ, 0u),
            bam_term_desc(clock_h, BAM_ACCESS_RESOURCE_READ, 0u),
        };

        BamQueryDesc query_desc{};
        query_desc.struct_size = static_cast<uint32_t>(sizeof(BamQueryDesc));
        query_desc.terms       = terms;
        query_desc.term_count  = 5u;

        BamSystemDesc sys_desc{};
        sys_desc.struct_size = static_cast<uint32_t>(sizeof(BamSystemDesc));
        sys_desc.name        = "CppBounceGravity";
        sys_desc.queries     = &query_desc;
        sys_desc.query_count = 1u;
        sys_desc.callback    = &CppGravityInstance::bounce_trampoline;
        sys_desc.user_data   = this;
        sys_desc.schedule    = BAM_SCHEDULE_UPDATE;

        return api_.register_system(load_ctx, &sys_desc);
    }

private:
    static BamResult bounce_trampoline(BamSystemCtx* sys_ctx, void* user_data) noexcept {
        if (!sys_ctx || !user_data) {
            return BAM_RESULT_NULL_POINTER;
        }
        try {
            return static_cast<CppGravityInstance*>(user_data)->run_bounce(sys_ctx);
        } catch (...) {
            return BAM_RESULT_PANIC_CAUGHT;
        }
    }

    BamResult run_bounce(BamSystemCtx* sys_ctx) {
        BamIter* iter = nullptr;
        BamResult res = api_.sys_query(sys_ctx, 0u, &iter);
        if (res != BAM_RESULT_OK) {
            return res;
        }

        BamColumnInfo xf_info{}, mot_info{}, vis_info{}, meta_info{};
        if (api_.iter_column_info(iter, 0u, &xf_info) != BAM_RESULT_OK ||
            api_.iter_column_info(iter, 1u, &mot_info) != BAM_RESULT_OK ||
            api_.iter_column_info(iter, 2u, &vis_info) != BAM_RESULT_OK ||
            api_.iter_column_info(iter, 3u, &meta_info) != BAM_RESULT_OK) {
            return BAM_RESULT_TYPE_MISMATCH;
        }

        if (xf_info.size != sizeof(StudioTransform3D) ||
            mot_info.size != sizeof(StudioMotion3D) ||
            vis_info.size != sizeof(StudioVisualStyle) ||
            meta_info.size != sizeof(StudioActorMeta)) {
            return BAM_RESULT_TYPE_MISMATCH;
        }

        float dt = 1.0f / 60.0f;
        float t = 0.0f;
        float amp = 1.0f;
        const auto* clock_ptr = reinterpret_cast<const StudioSimClock*>(api_.res_ptr(iter, 4u));
        if (clock_ptr != nullptr) {
            dt = clock_ptr->dt * clock_ptr->speed_scale;
            t = clock_ptr->elapsed * clock_ptr->speed_scale;
            amp = clock_ptr->amplitude;
        } else {
            local_ticks_ += 1u;
            t = static_cast<float>(local_ticks_) * dt;
        }
        if (dt < 0.0f) dt = 0.0f;
        if (dt > 0.05f) dt = 0.05f;

        const float gravity = 15.5f * (0.7f + 0.3f * amp);
        size_t count = 0u;

        while (api_.iter_next_chunk(iter, &count) != BAM_FALSE && count > 0u) {
            auto* xfs = reinterpret_cast<StudioTransform3D*>(api_.iter_column_mutable(iter, 0u));
            auto* mots = reinterpret_cast<StudioMotion3D*>(api_.iter_column_mutable(iter, 1u));
            auto* viss = reinterpret_cast<StudioVisualStyle*>(api_.iter_column_mutable(iter, 2u));
            const auto* metas =
                reinterpret_cast<const StudioActorMeta*>(api_.iter_column_readonly(iter, 3u));

            if (!xfs || !mots || !viss || !metas) {
                return BAM_RESULT_NULL_POINTER;
            }

            for (size_t i = 0u; i < count; ++i) {
                if ((metas[i].flags & 1u) != 0u) {
                    continue;
                }

                mots[i].vy -= gravity * dt;
                xfs[i].py += mots[i].vy * dt;

                if (xfs[i].py <= 0.45f) {
                    xfs[i].py = 0.45f;
                    const float wave =
                        std::fabs(std::sin(t * 1.9f + mots[i].phase * 1.37f));
                    mots[i].vy = (5.2f + 3.4f * wave) * amp;
                    // Inject kinetic bounce pulse + energy for downstream scale/color systems!
                    viss[i].pulse = std::fmin(2.0f, viss[i].pulse + 0.85f);
                    viss[i].energy = std::fmin(2.0f, viss[i].energy + 0.55f);
                }
            }
        }

        return BAM_RESULT_OK;
    }

    BamHostApi api_;
    uint64_t local_ticks_ = 0u;
};

} // namespace

extern "C" {

BAM_EXPORT const BamManifest* bamabi_manifest(void) {
    return &kManifest;
}

BAM_EXPORT BamResult bamabi_init(
    const BamHostApi* host_api,
    BamLoadCtx*       load_ctx,
    BamInstance**     instance_out
) {
    if (!load_ctx || !instance_out) {
        return BAM_RESULT_NULL_POINTER;
    }
    *instance_out = nullptr;

    try {
        BamHostApi local_api{};
        BamResult res = bam_host_api_copy_bounded(&local_api, sizeof(local_api), host_api);
        if (res != BAM_RESULT_OK) {
            return res;
        }

        auto instance = std::make_unique<CppGravityInstance>(local_api);
        res = instance->init(load_ctx);
        if (res != BAM_RESULT_OK) {
            return res;
        }

        *instance_out = reinterpret_cast<BamInstance*>(instance.release());
        return BAM_RESULT_OK;
    } catch (...) {
        return BAM_RESULT_PANIC_CAUGHT;
    }
}

BAM_EXPORT void bamabi_shutdown(BamInstance* instance) {
    if (instance != nullptr) {
        try {
            delete reinterpret_cast<CppGravityInstance*>(instance);
        } catch (...) {
            // Never unwind across extern "C".
        }
    }
}

} // extern "C"
