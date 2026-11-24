ir 1;

// Only `Present` is a JS global: `Absent` has no value at runtime.
class Present {}
class Absent {}

absents(): arr<Absent>;
presents(): arr<Present>;
