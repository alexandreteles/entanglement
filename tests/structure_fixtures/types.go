package fixture

type Base struct{}

func (base Base) Value() {}
func (base *Base) Pointer() {}

type Outer struct{ Base }
type PointerOuter struct{ *Base }

type Requirement interface { Value() }

type Scalar interface { ~int | ~string }
type Collection[Item Scalar] struct { Values []Item }
