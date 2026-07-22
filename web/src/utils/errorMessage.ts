export function errorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : "请求失败";
}
