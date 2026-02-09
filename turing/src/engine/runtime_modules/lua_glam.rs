use crate::interop::params::Param;
use anyhow::{Result, anyhow};
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3, Vec4};
use mlua::prelude::{LuaError, LuaMultiValue};
use mlua::{
    FromLua, FromLuaMulti, IntoLua, IntoLuaMulti, Lua, MaybeSend, Table, UserData, UserDataMethods,
    Value,
};

#[derive(Clone, Copy)]
pub(crate) struct LuaVec2(pub Vec2);
#[derive(Clone, Copy)]
pub(crate) struct LuaVec3(pub Vec3);
#[derive(Clone, Copy)]
pub(crate) struct LuaVec4(pub Vec4);
#[derive(Clone, Copy)]
pub(crate) struct LuaQuat(pub Quat);
#[derive(Clone, Copy)]
pub(crate) struct LuaMat4(pub Mat4);

fn to_f32(v: &Value) -> Option<f32> {
    match v {
        Value::Integer(i) => Some(*i as f32),
        Value::Number(f) => Some(*f as f32),
        _ => None,
    }
}

macro_rules! from_lua {
    ( $($typ:path ),+ ) => {
        $(impl FromLua for $typ {
            fn from_lua(value: Value, _: &Lua) -> mlua::Result<Self> {
                match value {
                    Value::UserData(u) => {
                        Ok(*u.borrow::<Self>()?)
                    }
                    _ => Err(mlua::Error::runtime("value is not a LuaVec2"))
                }
            }
        })+
    };
}

from_lua! {
    LuaVec2, LuaVec3, LuaVec4,
    LuaQuat, LuaMat4
}

macro_rules! do_math_op {
    (
        $this:expr, $args:ident, $lua_ty:path,
        $op:tt $([$rs_ctor:path, $( $part:tt ),+ ])?
        $( (scalar: $scalar_ctor:path) )?
        ? $err:expr
    ) => {
        {
            let $args = $args.into_vec();
            match $args.as_slice() {
                [Value::UserData(u)] => {
                    let other = u.borrow::<$lua_ty>()?.0;
                    Ok($lua_ty($this.0 $op other))
                }
                $([$( $part ),+] => {
                    let ( $( Some($part) ),+ ) = ( $(to_f32($part)),+ ) else {
                        return Err(mlua::Error::runtime($err));
                    };
                    Ok($lua_ty($this.0 $op $rs_ctor( $( $part ),+ )))
                })?
                $([Value::Integer(i)] => Ok($lua_ty($this.0 $op $scalar_ctor(*i as f32))),
                [Value::Number(f)] => Ok($lua_ty($this.0 $op $scalar_ctor(*f as f32))),)?
                _ => Err(mlua::Error::runtime($err))
            }
        }
    };
}

macro_rules! math_fns {
    (
        $lua_ty:path,
        $( $name:ident $op:tt $([$rs_ctor:path, $( $part:tt ),+ ])? $((scalar: $scalar_ctor:path))? );+ $(;)?
        ? $err:expr
        $(; - $neg_name:ident)? $(; == $eq_name:ident)? $(;)?
    ) => {
        $(fn $name (_: &Lua, this: &$lua_ty, args: LuaMultiValue) -> mlua::Result<$lua_ty> {
            do_math_op!(this, args, $lua_ty, $op $([$rs_ctor, $( $part ),+ ])? $((scalar: $scalar_ctor))? ? $err )
        })+
        $(fn $neg_name (_: &Lua, this: &$lua_ty, _: ()) -> mlua::Result<$lua_ty> {
            Ok($lua_ty(-this.0))
        })?
        $(fn $eq_name (_: &Lua, this: &$lua_ty, other: $lua_ty) -> mlua::Result<bool> {
            Ok(this.0 == other.0)
        })?
    };
}

math_fns! {
    LuaVec2,
    add_v2 + [Vec2::new, x, y] (scalar: Vec2::splat);
    sub_v2 - [Vec2::new, x, y] (scalar: Vec2::splat);
    mul_v2 * [Vec2::new, x, y] (scalar: Vec2::splat);
    div_v2 / [Vec2::new, x, y] (scalar: Vec2::splat);
    ? "Expected Vec2 or 2 numbers";
    - neg_v2;
    == eq_v2;
}

math_fns! {
    LuaVec3,
    add_v3 + [Vec3::new, x, y, z] (scalar: Vec3::splat);
    sub_v3 - [Vec3::new, x, y, z] (scalar: Vec3::splat);
    mul_v3 * [Vec3::new, x, y, z] (scalar: Vec3::splat);
    div_v3 / [Vec3::new, x, y, z] (scalar: Vec3::splat);
    ? "Expected Vec3 or 3 numbers";
    - neg_v3;
    == eq_v3;
}

math_fns! {
    LuaVec4,
    add_v4 + [Vec4::new, x, y, z, w] (scalar: Vec4::splat);
    sub_v4 - [Vec4::new, x, y, z, w] (scalar: Vec4::splat);
    mul_v4 * [Vec4::new, x, y, z, w] (scalar: Vec4::splat);
    div_v4 / [Vec4::new, x, y, z, w] (scalar: Vec4::splat);
    ? "Expected Vec4 or 4 numbers";
    - neg_v4;
    == eq_v4;
}

math_fns! {
    LuaQuat,
    add_q + [Quat::from_xyzw, x, y, z, w];
    sub_q - [Quat::from_xyzw, x, y, z, w];
    ? "Expected Quat or 4 numbers";
    - neg_q;
    == eq_q;
}

fn mul_q(lua: &Lua, this: &LuaQuat, args: LuaMultiValue) -> mlua::Result<Value> {
    let args = args.into_vec();
    match args.as_slice() {
        [Value::UserData(u)] => {
            if let Ok(quat) = u.borrow::<LuaQuat>() {
                LuaQuat(this.0 * quat.0).into_lua(lua)
            } else if let Ok(vec3) = u.borrow::<LuaVec3>() {
                let vec3 = this.0 * vec3.0;
                LuaVec3(vec3).into_lua(lua)
            } else if let Ok(mat4) = u.borrow::<LuaMat4>() {
                let lhs = Mat4::from_quat(this.0);
                let mat4 = lhs * mat4.0;
                LuaMat4(mat4).into_lua(lua)
            } else {
                Err(mlua::Error::runtime("Expected a Quat, Vec3, or Mat4"))
            }
        }
        _ => Err(mlua::Error::runtime("Expected a Quat, Vec3, or Mat4")),
    }
}

fn mul_m4(lua: &Lua, this: &LuaMat4, args: LuaMultiValue) -> mlua::Result<Value> {
    let args = args.into_vec();
    match args.as_slice() {
        [Value::UserData(u)] => {
            if let Ok(mat4) = u.borrow::<LuaMat4>() {
                LuaMat4(this.0 * mat4.0).into_lua(lua)
            } else if let Ok(vec4) = u.borrow::<LuaVec4>() {
                LuaVec4(this.0 * vec4.0).into_lua(lua)
            } else if let Ok(quat) = u.borrow::<LuaQuat>() {
                let quat = Mat4::from_quat(quat.0);
                LuaMat4(this.0 * quat).into_lua(lua)
            } else {
                Err(mlua::Error::runtime("Expected a Mat4, Vec4, or Quat"))
            }
        }
        _ => Err(mlua::Error::runtime("Expected a Mat4, Vec4, or Quat")),
    }
}

fn eq_m4(_: &Lua, this: &LuaMat4, other: LuaMat4) -> mlua::Result<bool> {
    Ok(this.0 == other.0)
}

macro_rules! vec_methods {
    ( $methods:expr, $lua_ty:path) => {{
        $methods.add_method("length", |_, this, _: ()| Ok(this.0.length()));
        $methods.add_method("normalize", |_, this, _: ()| {
            Ok($lua_ty(this.0.normalize()))
        });
        $methods.add_method("dot", |_, this, rhs: $lua_ty| Ok(this.0.dot(rhs.0)));
        $methods.add_method("distance", |_, this, rhs: $lua_ty| {
            Ok(this.0.distance(rhs.0))
        });
        $methods.add_method("clamp", |_, this, (min, max): ($lua_ty, $lua_ty)| {
            Ok($lua_ty(this.0.clamp(min.0, max.0)))
        });
        $methods.add_method("clamp_length", |_, this, (min, max): (f32, f32)| {
            Ok($lua_ty(this.0.clamp_length(min, max)))
        });
        $methods.add_method("abs", |_, this, _: ()| Ok($lua_ty(this.0.abs())));
        $methods.add_method("ceil", |_, this, _: ()| Ok($lua_ty(this.0.ceil())));
        $methods.add_method("floor", |_, this, _: ()| Ok($lua_ty(this.0.floor())));
        $methods.add_method("fract", |_, this, _: ()| Ok($lua_ty(this.0.fract())));
        $methods.add_method("lerp", |_, this, (rhs, t): ($lua_ty, f32)| {
            Ok($lua_ty(this.0.lerp(rhs.0, t)))
        });
        $methods.add_method("length_squared", |_, this, _: ()| {
            Ok(this.0.length_squared())
        });
        $methods.add_method("midpoint", |_, this, rhs: $lua_ty| {
            Ok($lua_ty(this.0.midpoint(rhs.0)))
        });
        $methods.add_method("copy", |_, this, _: ()| Ok($lua_ty(this.0)));
    }};
}

impl UserData for LuaVec2 {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method("__add", add_v2);
        methods.add_method("add", add_v2);

        methods.add_meta_method("__mul", mul_v2);
        methods.add_method("mul", mul_v2);

        methods.add_meta_method("__sub", sub_v2);
        methods.add_method("sub", sub_v2);

        methods.add_meta_method("__div", div_v2);
        methods.add_method("div", div_v2);

        methods.add_meta_method("__neg", neg_v2);
        methods.add_meta_method("__eq", eq_v2);

        vec_methods!(methods, LuaVec2);
        methods.add_method("angle_to", |_, this, rhs: LuaVec2| {
            Ok(this.0.angle_to(rhs.0))
        });
        methods.add_method("extend", |_, this, z: f32| Ok(LuaVec3(this.0.extend(z))));
    }
}

impl UserData for LuaVec3 {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method("__add", add_v3);
        methods.add_method("add", add_v3);

        methods.add_meta_method("__mul", mul_v3);
        methods.add_method("mul", mul_v3);

        methods.add_meta_method("__sub", sub_v3);
        methods.add_method("sub", sub_v3);

        methods.add_meta_method("__div", div_v3);
        methods.add_method("div", div_v3);

        methods.add_meta_method("__neg", neg_v3);
        methods.add_meta_method("__eq", eq_v3);

        vec_methods!(methods, LuaVec3);

        methods.add_method("extend", |_, this, w: f32| Ok(LuaVec4(this.0.extend(w))));

        methods.add_method("angle_between", |_, this, rhs: LuaVec3| {
            Ok(this.0.angle_between(rhs.0))
        });
    }
}

impl UserData for LuaVec4 {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method("__add", add_v4);
        methods.add_method("add", add_v4);

        methods.add_meta_method("__mul", mul_v4);
        methods.add_method("mul", mul_v4);

        methods.add_meta_method("__sub", sub_v4);
        methods.add_method("sub", sub_v4);

        methods.add_meta_method("__div", div_v4);
        methods.add_method("div", div_v4);

        methods.add_meta_method("__neg", neg_v4);
        methods.add_meta_method("__eq", eq_v4);

        vec_methods!(methods, LuaVec4);

        methods.add_method("truncate", |_, this, _: ()| Ok(LuaVec3(this.0.truncate())))
    }
}

impl UserData for LuaQuat {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method("__add", add_q);
        methods.add_method("add", add_q);

        methods.add_meta_method("__mul", mul_q);
        methods.add_method("mul", mul_q);

        methods.add_meta_method("__sub", sub_q);
        methods.add_method("sub", sub_q);

        methods.add_meta_method("__neg", neg_q);
        methods.add_meta_method("__eq", eq_q);

        methods.add_method("copy", |_, this, _: ()| Ok(LuaQuat(this.0)));
    }
}

impl UserData for LuaMat4 {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method("__mul", mul_m4);
        methods.add_method("mul", mul_m4);

        methods.add_meta_method("__eq", eq_m4);

        methods.add_method("copy", |_, this, _: ()| Ok(LuaMat4(this.0)));
    }
}

#[inline]
fn any_err(err: LuaError) -> anyhow::Error {
    anyhow!("{err}")
}

pub fn create_class_tables(lua: &Lua, api: &Table) -> Result<()> {
    create_vec2_tables(lua, api)?;
    create_vec3_tables(lua, api)?;
    create_vec4_tables(lua, api)?;
    create_quat_tables(lua, api)?;
    create_mat4_tables(lua, api)?;

    Ok(())
}

trait TableExtension {
    fn create_function<F, A, R>(&self, lua: &Lua, name: impl IntoLua, func: F) -> Result<()>
    where
        F: Fn(&Lua, A) -> mlua::Result<R> + MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti;
}

impl TableExtension for Table {
    #[inline]
    fn create_function<F, A, R>(&self, lua: &Lua, name: impl IntoLua, func: F) -> Result<()>
    where
        F: Fn(&Lua, A) -> mlua::Result<R> + MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {
        self.set(name, lua.create_function(func).map_err(any_err)?)
            .map_err(any_err)
    }
}

fn create_vec2_tables(lua: &Lua, api: &Table) -> Result<()> {
    let class = lua.create_table().map_err(any_err)?;

    class.create_function(lua, "new", |_, (x, y): (f32, f32)| {
        Ok(LuaVec2(Vec2::new(x, y)))
    })?;

    class.create_function(lua, "zero", |_, _: ()| Ok(LuaVec2(Vec2::ZERO)))?;

    class.create_function(lua, "splat", |_, v: f32| Ok(LuaVec2(Vec2::splat(v))))?;

    api.raw_set("Vec2", class).map_err(any_err)
}

fn create_vec3_tables(lua: &Lua, api: &Table) -> Result<()> {
    let class = lua.create_table().map_err(any_err)?;

    class.create_function(lua, "new", |_, (x, y, z): (f32, f32, f32)| {
        Ok(LuaVec3(Vec3::new(x, y, z)))
    })?;

    class.create_function(lua, "zero", |_, _: ()| Ok(LuaVec3(Vec3::ZERO)))?;

    class.create_function(lua, "splat", |_, v: f32| Ok(LuaVec3(Vec3::splat(v))))?;

    class.create_function(lua, "from_homogeneous", |_, v4: LuaVec4| {
        if v4.0.w == 0. {
            Err(mlua::Error::runtime(
                "Cannot Create Vec3 from Vec4 with w == 0",
            ))
        } else {
            Ok(LuaVec3(Vec3::from_homogeneous(v4.0)))
        }
    })?;

    api.raw_set("Vec3", class).map_err(any_err)
}

fn create_vec4_tables(lua: &Lua, api: &Table) -> Result<()> {
    let class = lua.create_table().map_err(any_err)?;

    class.create_function(lua, "new", |_, (x, y, z, w): (f32, f32, f32, f32)| {
        Ok(LuaVec4(Vec4::new(x, y, z, w)))
    })?;

    class.create_function(lua, "zero", |_, _: ()| Ok(LuaVec4(Vec4::ZERO)))?;

    class.create_function(lua, "splat", |_, v: f32| Ok(LuaVec4(Vec4::splat(v))))?;

    api.raw_set("Vec4", class).map_err(any_err)
}

fn create_quat_tables(lua: &Lua, api: &Table) -> Result<()> {
    let class = lua.create_table().map_err(any_err)?;

    macro_rules! ok {
        ($q:expr) => {
            Ok(LuaQuat($q))
        };
    }

    macro_rules! fun {
        ( $name:literal, $f:expr ) => {
            class.create_function(lua, $name, $f)?;
        };
    }

    fun!("identity", |_, _: ()| { ok!(Quat::IDENTITY) });

    fun!("from_xyzw", |_, (x, y, z, w): (f32, f32, f32, f32)| {
        ok!(Quat::from_xyzw(x, y, z, w))
    });

    fun!("from_vec4", |_, v4: LuaVec4| { ok!(Quat::from_vec4(v4.0)) });

    fun!("from_axis_angle", |_, (axis, angle): (LuaVec3, f32)| {
        ok!(Quat::from_axis_angle(axis.0, angle))
    });

    fun!("from_scaled_axis", |_, v: LuaVec3| {
        ok!(Quat::from_scaled_axis(v.0))
    });

    fun!("from_rotation_x", |_, x: f32| {
        ok!(Quat::from_rotation_x(x))
    });

    fun!("from_rotation_y", |_, y: f32| {
        ok!(Quat::from_rotation_y(y))
    });

    fun!("from_rotation_z", |_, z: f32| {
        ok!(Quat::from_rotation_z(z))
    });

    fun!(
        "from_euler",
        |_, (euler, x, y, z): (String, f32, f32, f32)| {
            let euler = match euler.to_uppercase().as_str() {
                "XYZ" => EulerRot::XYZ,
                "XZY" => EulerRot::XZY,
                "YXZ" => EulerRot::YXZ,
                "YZX" => EulerRot::YZX,
                "ZXY" => EulerRot::ZXY,
                "ZYX" => EulerRot::ZYX,
                _ => {
                    return Err(mlua::Error::runtime(format!(
                        "Invalid euler ordering: {}",
                        euler
                    )));
                }
            };

            ok!(Quat::from_euler(euler, x, y, z))
        }
    );

    fun!("look_to_lh", |_, (dir, up): (LuaVec3, LuaVec3)| {
        ok!(Quat::look_to_lh(dir.0, up.0))
    });

    fun!("look_to_rh", |_, (dir, up): (LuaVec3, LuaVec3)| {
        ok!(Quat::look_to_rh(dir.0, up.0))
    });

    fun!("look_at_lh", |_,
                        (eye, center, up): (
        LuaVec3,
        LuaVec3,
        LuaVec3
    )| {
        ok!(Quat::look_at_lh(eye.0, center.0, up.0))
    });

    fun!("look_at_rh", |_,
                        (eye, center, up): (
        LuaVec3,
        LuaVec3,
        LuaVec3
    )| {
        ok!(Quat::look_at_rh(eye.0, center.0, up.0))
    });

    api.raw_set("Quat", class).map_err(any_err)
}

fn create_mat4_tables(lua: &Lua, api: &Table) -> Result<()> {
    let class = lua.create_table().map_err(any_err)?;

    macro_rules! ok {
        ($m:expr) => {
            Ok(LuaMat4($m))
        };
    }

    macro_rules! fun {
        ( $name:literal, $f:expr ) => {
            class.create_function(lua, $name, $f)?;
        };
    }

    fun!("identity", |_, _: ()| { ok!(Mat4::IDENTITY) });

    fun!("from_cols", |_,
                       (c0, c1, c2, c3): (
        LuaVec4,
        LuaVec4,
        LuaVec4,
        LuaVec4
    )| {
        ok!(Mat4::from_cols(c0.0, c1.0, c2.0, c3.0))
    });

    fun!("from_diagonal", |_, v: LuaVec4| {
        ok!(Mat4::from_diagonal(v.0))
    });

    fun!(
        "from_scale_rotation_translation",
        |_, (scale, rot, trans): (LuaVec3, LuaQuat, LuaVec3)| {
            ok!(Mat4::from_scale_rotation_translation(
                scale.0, rot.0, trans.0
            ))
        }
    );

    fun!("from_translation", |_, v: LuaVec3| {
        ok!(Mat4::from_translation(v.0))
    });

    fun!("from_scale", |_, v: LuaVec3| { ok!(Mat4::from_scale(v.0)) });

    fun!("from_uniform_scale", |_, s: f32| {
        ok!(Mat4::from_scale(Vec3::splat(s)))
    });

    fun!("from_quat", |_, q: LuaQuat| { ok!(Mat4::from_quat(q.0)) });

    fun!("from_axis_angle", |_, (axis, angle): (LuaVec3, f32)| {
        ok!(Mat4::from_axis_angle(axis.0, angle))
    });

    fun!("from_rotation_x", |_, angle: f32| {
        ok!(Mat4::from_rotation_x(angle))
    });

    fun!("from_rotation_y", |_, angle: f32| {
        ok!(Mat4::from_rotation_y(angle))
    });

    fun!("from_rotation_z", |_, angle: f32| {
        ok!(Mat4::from_rotation_z(angle))
    });

    fun!("look_at_lh", |_,
                        (eye, center, up): (
        LuaVec3,
        LuaVec3,
        LuaVec3
    )| {
        ok!(Mat4::look_at_lh(eye.0, center.0, up.0))
    });

    fun!("look_at_rh", |_,
                        (eye, center, up): (
        LuaVec3,
        LuaVec3,
        LuaVec3
    )| {
        ok!(Mat4::look_at_rh(eye.0, center.0, up.0))
    });

    fun!("look_to_lh", |_,
                        (eye, dir, up): (
        LuaVec3,
        LuaVec3,
        LuaVec3
    )| {
        ok!(Mat4::look_to_lh(eye.0, dir.0, up.0))
    });

    fun!("look_to_rh", |_,
                        (eye, dir, up): (
        LuaVec3,
        LuaVec3,
        LuaVec3
    )| {
        ok!(Mat4::look_to_rh(eye.0, dir.0, up.0))
    });

    fun!("perspective_lh", |_,
                            (fov_y, aspect, near, far): (
        f32,
        f32,
        f32,
        f32
    )| {
        ok!(Mat4::perspective_lh(fov_y, aspect, near, far))
    });

    fun!("perspective_rh", |_,
                            (fov_y, aspect, near, far): (
        f32,
        f32,
        f32,
        f32
    )| {
        ok!(Mat4::perspective_rh(fov_y, aspect, near, far))
    });

    fun!("perspective_infinite_lh", |_,
                                     (fov_y, aspect, near): (
        f32,
        f32,
        f32
    )| {
        ok!(Mat4::perspective_infinite_lh(fov_y, aspect, near))
    });

    fun!("perspective_infinite_rh", |_,
                                     (fov_y, aspect, near): (
        f32,
        f32,
        f32
    )| {
        ok!(Mat4::perspective_infinite_rh(fov_y, aspect, near))
    });

    fun!("perspective_infinite_reverse_lh", |_,
                                             (
        fov_y,
        aspect,
        near,
    ): (
        f32,
        f32,
        f32
    )| {
        ok!(Mat4::perspective_infinite_reverse_lh(fov_y, aspect, near))
    });

    fun!("perspective_reverse_lh", |_,
                                    (fov_y, aspect, near): (
        f32,
        f32,
        f32
    )| {
        ok!(Mat4::perspective_infinite_reverse_rh(fov_y, aspect, near))
    });

    fun!("orthographic_lh", |_,
                             (
        left,
        right,
        bottom,
        top,
        near,
        far,
    ): (
        f32,
        f32,
        f32,
        f32,
        f32,
        f32
    )| {
        ok!(Mat4::orthographic_lh(left, right, bottom, top, near, far))
    });

    fun!("orthographic_rh", |_,
                             (
        left,
        right,
        bottom,
        top,
        near,
        far,
    ): (
        f32,
        f32,
        f32,
        f32,
        f32,
        f32
    )| {
        ok!(Mat4::orthographic_rh(left, right, bottom, top, near, far))
    });

    api.raw_set("Mat4", class).map_err(any_err)
}

pub fn create_vec2(v: Vec2, lua: &Lua) -> mlua::Result<Value> {
    Ok(Value::UserData(lua.create_userdata(LuaVec2(v))?))
}

pub fn create_vec3(v: Vec3, lua: &Lua) -> mlua::Result<Value> {
    Ok(Value::UserData(lua.create_userdata(LuaVec3(v))?))
}

pub fn create_vec4(v: Vec4, lua: &Lua) -> mlua::Result<Value> {
    Ok(Value::UserData(lua.create_userdata(LuaVec4(v))?))
}

pub fn create_quat(q: Quat, lua: &Lua) -> mlua::Result<Value> {
    Ok(Value::UserData(lua.create_userdata(LuaQuat(q))?))
}

pub fn create_mat4(m: Mat4, lua: &Lua) -> mlua::Result<Value> {
    Ok(Value::UserData(lua.create_userdata(LuaMat4(m))?))
}

pub fn unpack_vec2(v: Value) -> Param {
    match v {
        Value::UserData(d) => match d.borrow::<LuaVec2>() {
            Ok(v) => Param::Vec2(v.0),
            Err(e) => Param::Error(format!(
                "Expected LuaVec2 userdata, got different UserData: {e}"
            )),
        },
        other => Param::Error(format!("Expected Vec2 userdata, got {}", other.type_name())),
    }
}

pub fn unpack_vec3(v: Value) -> Param {
    match v {
        Value::UserData(d) => match d.borrow::<LuaVec3>() {
            Ok(v) => Param::Vec3(v.0),
            Err(e) => Param::Error(format!(
                "Expected LuaVec3 userdata, got different UserData: {e}"
            )),
        },
        other => Param::Error(format!("Expected Vec3 userdata, got {}", other.type_name())),
    }
}

pub fn unpack_vec4(v: Value) -> Param {
    match v {
        Value::UserData(d) => match d.borrow::<LuaVec4>() {
            Ok(v) => Param::Vec4(v.0),
            Err(e) => Param::Error(format!(
                "Expected LuaVec4 userdata, got different UserData: {e}"
            )),
        },
        other => Param::Error(format!("Expected Vec4 userdata, got {}", other.type_name())),
    }
}

pub fn unpack_quat(v: Value) -> Param {
    match v {
        Value::UserData(d) => match d.borrow::<LuaQuat>() {
            Ok(v) => Param::Quat(v.0),
            Err(e) => Param::Error(format!(
                "Expected LuaQuat userdata, got different UserData: {e}"
            )),
        },
        other => Param::Error(format!("Expected Quat userdata, got {}", other.type_name())),
    }
}

pub fn unpack_mat4(v: Value) -> Param {
    match v {
        Value::UserData(d) => match d.borrow::<LuaMat4>() {
            Ok(v) => Param::Mat4(v.0),
            Err(e) => Param::Error(format!(
                "Expected LuaMat4 userdata, got different UserData: {e}"
            )),
        },
        other => Param::Error(format!("Expected Mat4 userdata, got {}", other.type_name())),
    }
}
