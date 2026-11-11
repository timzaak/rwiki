// Separator-safe encoding of a channel ID set (a plain join(',') key would
// collide for IDs containing the separator, e.g. ["a","b"] vs ["a,b"]).
export function channelKeyOf(channelIds: readonly string[]): string {
  return JSON.stringify([...channelIds].sort())
}
