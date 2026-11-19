export async function message(text: string): Promise<void> {
  window.alert(text);
}

export async function ask(text: string): Promise<boolean> {
  return window.confirm(text);
}

export async function confirm(text: string): Promise<boolean> {
  return window.confirm(text);
}

export async function open(): Promise<null> {
  return null;
}

export async function save(): Promise<null> {
  return null;
}
