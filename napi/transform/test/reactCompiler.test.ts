import { describe, expect, it } from "vitest";

import { transformSync } from "../index";

// A single fixture exercising every concern the React Compiler integration has to
// handle together: a memoizable component using a hook, TypeScript types, JSX, ES
// module syntax, and top-level comments. The compiler runs first on the pristine
// AST, then the rest of the transform pipeline (TypeScript, JSX) runs on its
// output, and codegen emits the result.
const fixture = `// @license MIT
import { useState } from "react";

interface Props {
  text: string;
  onClick: () => void;
}

// Memoized component: exercises hooks, TS types, JSX and comments.
export function Component(props: Props) {
  const [count, setCount] = useState<number>(0);
  return (
    <div onClick={() => props.onClick()}>
      {props.text}: {count}
    </div>
  );
}
`;

describe("reactCompiler", () => {
  it("memoizes, composes with the TS + JSX transforms, and preserves comments", () => {
    const { code, errors } = transformSync("Component.tsx", fixture, {
      reactCompiler: true,
      jsx: { runtime: "automatic" },
    });

    expect(errors).toEqual([]);

    // React Compiler memoized the component.
    expect(code).toContain("react/compiler-runtime");
    expect(code).toContain("_c(");

    // JSX was lowered via the automatic runtime — no raw JSX remains.
    expect(code).toContain("jsx");
    expect(code).not.toContain("<div");

    // TypeScript was stripped: the interface, annotations and generic are gone.
    expect(code).not.toContain("interface Props");
    expect(code).not.toContain(": Props");
    expect(code).not.toContain("<number>");

    // The hook call and ES module syntax survive.
    expect(code).toContain("useState(");
    expect(code).toContain("export function Component");

    // Top-level comments survive react_compiler -> transformer -> codegen.
    expect(code).toContain("@license MIT");
    expect(code).toContain("Memoized component");
  });

  it("accepts a ReactCompilerOptions object", () => {
    const { code } = transformSync("Component.tsx", fixture, {
      reactCompiler: { compilationMode: "all" },
    });
    expect(code).toContain("react/compiler-runtime");
    expect(code).toContain("_c(");
  });

  it("does nothing when `reactCompiler` is omitted (the default) or `false`", () => {
    for (const options of [{}, { reactCompiler: false }]) {
      const { code } = transformSync("Component.tsx", fixture, options);
      expect(code).not.toContain("react/compiler-runtime");
      expect(code).not.toContain("_c(");
    }
  });
});
