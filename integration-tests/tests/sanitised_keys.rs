#![cfg(target_arch = "wasm32")]

use ts_gen_integration_tests::sanitised_keys::{
    ajax, ajax_2, collide_args, compare, dollar, dollar_2, dollar_dollar, each, fetch_url, ns,
    scoped, underscore_underscore, url, version, Collide, CollideInit, DollarUnderscore,
    MethodCollide, Op, Pair, ReadonlyCollide, ReservedClass, ReservedInit, ReservedMethod,
    SetterCollide, SetterNumbers, Sigil, Store,
};

// Compile-only coverage that wasm-bindgen accepts the `js_name` / `js_class`
// combinations emitted for sanitised names.
#[allow(dead_code)]
fn sanitised_bindings_compile(pair: &Pair) {
    let socket = pair._0();
    pair.set_0(&socket);
    let _ = Pair::_2();
    Pair::set_2(&socket);
    let _: String = pair.public();
    pair.set_public("x");
    let _: String = Pair::cfmodel();
    Pair::set_cfmodel("x");
    pair.emit("event");
    pair.cfrun();
    let _: Pair = Pair::create();

    let _ = Store::new();
    let _: Store = Store::create();
    let _: f64 = Store::count();
    Store::set_count(1.0);

    let _: Pair = dollar_2("#id");
    let _: String = version.with(|v| v.clone());

    let _: Vec<Pair> = dollar_dollar("#id");
    let _: f64 = underscore_underscore.with(|v| *v);
    each(1.0, "x");
    compare(Op::LtEq);
    let symbols = DollarUnderscore::new().unwrap();
    symbols.set_underscore(&symbols.underscore());
    let _ = [Sigil::Dollar, Sigil::Underscore];
}

#[allow(dead_code)]
fn colliding_names_compile(
    collide: &Collide,
    methods: &MethodCollide,
    readonly: &ReadonlyCollide,
    numbers: &SetterNumbers,
) {
    let _: String = collide.data();
    collide.set_data("x");
    let _: String = collide.data_2();
    collide.set_data_2("x");

    let init = CollideInit::builder(1.0, 2.0).data("a").data_2("b").build();
    let _: f64 = init.foo_bar() + init.foo_bar_2();
    init.set_foo_bar_2(3.0);
    let _ = CollideInit::new(1.0, 2.0);

    methods.data();
    let _: String = methods.data_2();
    methods.set_data_2("x");
    methods.set_foo();
    let _: String = methods.foo();
    methods.set_foo_2("x");
    methods.call();
    methods.call_with_a("x");
    methods.call_with_a_2(1.0);

    let setters = SetterCollide::new("a", "b");
    let _: String = setters.set_foo();
    setters.set_set_foo("x");
    let _: String = setters.foo();
    setters.set_foo_2("x");

    let _: String = numbers.data();
    numbers.set_data_3("x");
    let _: String = numbers.data_2();
    numbers.set_data_2("x");
    numbers.set_data();

    let _: String = readonly.value();
    readonly.set_value("x");
    let _: f64 = ReadonlyCollide::level();
    ReadonlyCollide::set_level(1.0);

    ajax("/a");
    ajax_2("/b");
    collide_args("a", 1.0, "b", 2.0);
    dollar();

    let reserved: ReservedInit = ReservedInit::builder("a", "b")
        .builder_(1.0)
        .build_(true)
        .build();
    let _: ReservedInit = ReservedInit::new("a", "b");
    let _: String = reserved.inner();
    let _: String = reserved.new_();
    reserved.set_new("b");
    let _: Option<f64> = reserved.builder_();
    let _: Option<bool> = reserved.build_();
    let class = ReservedClass::new().unwrap();
    let _: String = class.new_();
    class.set_new("x");
    ReservedMethod::new().unwrap().new_2();

    fetch_url(&url.with(|u| u.clone()));
    scoped::fetch_url("x");
    let _: f64 = ns::x.with(|x| *x);
    let _: String = ns::x_2.with(|x| x.clone());
}
