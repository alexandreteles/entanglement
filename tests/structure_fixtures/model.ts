export interface Requirement { execute(): void; }
export type Nested = Array<Array<string>>;
export interface Consumer extends Requirement { value: Nested; }
