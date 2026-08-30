/** Joins conditional class names; falsy entries are dropped. */
export function cx(...values: (string | false | undefined)[]): string {
  return values.filter((value): value is string => Boolean(value)).join(" ");
}
