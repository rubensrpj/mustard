<?php

namespace App\Support;

interface Identifiable
{
    public function id(): int;
}

trait HasLabel
{
    public string $label = '';

    public function label(): string
    {
        return $this->label;
    }
}

enum Status
{
    case Active;
    case Inactive;
}

function helper(): int
{
    return 1;
}
