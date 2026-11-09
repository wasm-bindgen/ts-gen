ir 1;

// A union of two union aliases: `Day` (a class, in the second) is tested
// before `ZonedFields` (any object, in the first).
class Zoned {
  get zone(): str;
}
class Day {
  get day(): f64;
}
interface ZonedFields {
  get zone(): str;
}
interface DayFields {
  get day(): f64;
}

enum DayLike {
  Day(Day);
  Zoned(Zoned);
  DayFields(DayFields);
  Str(str);
}
enum ZonedLike {
  Zoned(Zoned);
  ZonedFields(ZonedFields);
  Str(str);
}
enum ZonedLikeOrDayLike {
  ZonedLike(ZonedLike);
  DayLike(DayLike);
}

relativeTo(): ZonedLikeOrDayLike;
