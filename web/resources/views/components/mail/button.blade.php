@props(['href'])
<table role="presentation" cellpadding="0" cellspacing="0" border="0" style="margin:26px 0 8px">
    <tr>
        <td align="center" bgcolor="#6d5ae0" style="background:#6d5ae0;background-image:linear-gradient(180deg,#8a7cf5,#5a3fd6);border-radius:12px">
            <a href="{{ $href }}" style="display:inline-block;padding:13px 24px;font-size:14.5px;font-weight:600;color:#ffffff;text-decoration:none">{{ $slot }}</a>
        </td>
    </tr>
</table>
