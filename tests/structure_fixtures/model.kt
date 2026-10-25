interface Requirement { fun execute(): String }
class Consumer : Requirement { override fun execute(): String = "ready" }
