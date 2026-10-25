import typing
class Base:
    def execute(self, value: typing.Any) -> typing.Any:
        return value
class Child(Base):
    pass
