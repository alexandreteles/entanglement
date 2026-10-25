<?php trait Provided { public function execute() {} } class Consumer { use Provided { execute as aliasExecute; } }
