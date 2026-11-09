ir 1;
with std;

class Event {
  get stamp(): Date;
  at(d: Date): f64;
}
