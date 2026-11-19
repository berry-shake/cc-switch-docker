declare const __CC_SWITCH_VERSION__: string;

export async function getVersion(): Promise<string> {
  return __CC_SWITCH_VERSION__;
}

export async function getName(): Promise<string> {
  return "CC Switch Server";
}
